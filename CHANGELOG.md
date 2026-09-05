<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Changelog

All notable changes to Teksilo are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) —
though pre-1.0, so breaking changes can land in a minor bump. `release.toml`
keeps every workspace crate on one shared version; entries below are grouped
by crate for clarity, not because crates version independently.

## [Unreleased]

### Fixed

#### Automation

- A read deadline on the automation bridge's Unix socket now expires as
  `TimedOut` — the kind the transport trait documents and the Windows named
  pipe already returns — instead of the platform's `WouldBlock`.
- The short `/tmp` fallback the bridge takes when `$TMPDIR` would overflow
  `sun_path` no longer tries to make `/tmp` itself owner-only. It never could
  on macOS, where `/tmp` is a symlink the check rejects by design, so the
  fallback could not bind at all; on Linux the same call would chmod a `1777`
  `/tmp` to `0700`, which fails for a normal user and succeeds — for every
  other process on the machine — for one running as root. Only the bridge's
  own descriptor directory is tightened now; the per-process socket directory
  underneath is still created `0700`.

## [0.9.3] - 2026-09-04

Three unrelated strands: one keyboard contract for the five data views, MCP
automation on Windows and macOS, and a text field that stops painting a
selection it does not own.

### Added

#### Data views

See [docs/data-view-keyboard.md](docs/data-view-keyboard.md) for the full chord
table.

- `common::list_nav`: the edge-and-page chords as pure functions over
  `(key, modifiers, view kind)`, with `_for` twins so both platform branches
  are reachable from one host's test run.
- `SelectionModel::extend_to_additive`, and `Ctrl+Shift`+navigation on top of
  it: a second disjoint range can be built without losing the first.
- `Ctrl+Shift+A` deselects everything, in all five views.
- Tree expand chords in `TreeView` and `TreeTableView`: `*` expands a subtree,
  `+` and `-` one level, `→` on an open node moves to its first child.
- macOS aliases, all previously dead: `⌘↓` opens the focused row, `⌘↑`
  collapses or ascends, `⌥→` / `⌥←` expand or collapse a subtree.
- `PageUp` / `PageDown` in `ScrollBar` and `MenuList`; `Home` / `End` /
  `PageUp` / `PageDown` in `CommandPalette`.

#### Automation

- `teksilo-platform::automation_transport`, behind the `automation` feature:
  the per-OS endpoint the live bridge binds. Unix domain socket, Windows named
  pipe.
- `teksilo-automation::wire`: framing, token handshake and endpoint descriptor
  in pure `std` + `serde`, shared by both ends of the bridge.
- `install_automation_bridge_in_debug()` works on Linux, macOS and Windows, and
  is still a no-op in release on all three.
- The app publishes an endpoint descriptor at
  `<runtime dir>/teksilo-automation/<pid>.json`, so attaching needs nothing
  scraped from stderr.
- `--attach` (newest live app), `--attach-pid <pid>` and `--list` in
  `teksilo-automation-mcp`; `--connect <endpoint> --token <uuid>` remains as
  the explicit escape hatch.
- `command` modifier on `inject_key`, `inject_pointer` and `scroll`: the
  platform's primary accelerator, Control on Windows and Linux and ⌘ on macOS,
  where `ctrl` stays literal.
- Screenshots return a `{width, height, scale}` block beside the image. Pixels
  are physical; every other coordinate in the toolkit is logical.
- Error codes `GPU_READBACK_FAILED`, `BRIDGE_TIMEOUT`, `BRIDGE_DROPPED`,
  `BAD_REQUEST` and `BRIDGE_IO`, as constants rather than message strings.
- A `test-automation` CI job on Linux, macOS and Windows, driving a real window
  over the real endpoint and carrying the release canary.

#### Core

- `EventContext::focused()`, the widget that held focus when the event batch
  began. A widget-scoped shortcut is matched before the focused widget is
  offered the key, so a container that binds a key its own children also handle
  had no way to tell whether one of them was standing under it. Part of the
  same per-dispatch snapshot as the pointer position and the overlay bounds, so
  it reads `None` in a hand-made `EventContext`, and it is the focus as of
  dispatch time rather than a live read: a handler that has already called
  `request_focus` still sees the old value, which is the useful reading for one
  deciding whether to act at all.

### Changed

- **One GPU device per process**, shared by every window (`teksilo-platform`)
  and every offscreen renderer (`teksilo-render`), instead of one per window
  and one per caller. Each still gets its own surface and its own `Renderer`,
  so no caller sees another's cached glyphs; a window the shared adapter cannot
  present to gets its own device.
- `wait_for_condition` spends its budget as simulated frames rather than wall
  clock, so the same budget buys the same number of frames on every platform.
  The wall-clock backstop is 1× the budget, floor 250 ms. **Behaviour change.**
- The live bridge's reply wait is bounded at 15 s, reported as
  `BRIDGE_TIMEOUT`. **Behaviour change.**
- The transport's `probe` answers `Live` / `Busy` / `Dead` rather than a bool.
  **Behaviour change**: only an unambiguous absence unregisters an app.
- `try_read_texture_rgba` returns a `Result`, so a lost device costs one
  screenshot rather than the thread. **Behaviour change.**
- Offscreen renderers take the adapter's real limits rather than
  `downlevel_defaults`, which capped textures at 2048 where the path atlas
  grows to 4096.
- The Unix runtime directory on macOS is `$TMPDIR`, not the shared `/tmp`.
- `PlatformWindow::new` and `new_with_a11y` share their GPU and swapchain
  setup, and differ only in whether an AccessKit adapter is attached.

### Fixed

#### Data views

- **A `Shift` range could only grow.** `Shift+End` then `Shift+Home` selected
  the whole collection instead of reversing. Ranges are recomputed from a
  committed base. **Behaviour change** for anything driving
  `SelectionModel::extend_to` directly.
- **`Ctrl`+`Home` / `End` / `Page` moved the selection.** They move the cursor
  and leave the selection alone. **Behaviour change.**
- **`TableView`'s `Home` moved the column in every mode**, including the
  default `MultiRow`, which also made `Shift+Home` a no-op there. Scope follows
  the cursor topology. **Behaviour change.**
- **`GridView` paged by an estimated row height** and scrolled twice per
  keypress, and its `Home` / `End` stopped at the ends of a reflow row rather
  than the first and last tile. **Behaviour change.**
- **`GridView`'s `Ctrl+A` ignored the selection mode**, and its `Space` never
  toggled.
- **An open cell editor lost keys to the table**: `PageDown` inside an editor
  paged the cursor out from under the edit.
- **`TreeTableView`'s `←` was a dead key on every leaf**, and its expand /
  collapse arrows ignored the modifiers, so `Shift+→` opened a row instead of
  extending the selection.
- **`TreeView`'s expand / collapse arrows ignored the layout direction**, so a
  right-to-left tree collapsed on the wrong key.
- **Type-ahead could not reach an accented label**: case was folded with
  `to_ascii_lowercase`, which leaves `É` alone on both sides.
- **A selectable `TableView` exposed no selection to Windows assistive tech.**
  It announces `Role::Grid` now, and its cells `Role::GridCell` in a
  cell-selection mode. No effect on macOS or AT-SPI, where the roles coincide.
- **`TreeTableView` advertised an expand it never performed**, so Windows could
  ask a row to open and watch nothing happen.
- **`TableView`, `TreeTableView` and `GridView` ignored
  `Action::ScrollIntoView`**, so assistive tech had no way to bring a row or a
  tile into view.

#### Automation

- **Two concurrent offscreen renders killed the process** roughly a quarter of
  the time on a WARP host: two D3D12 WARP devices rasterizing at once fault
  inside `d3d10warp.dll`. Closed by the shared device above. The same fault was
  latent, never live, for windows.
- **`create_test_renderer` reported "no GPU" on machines that have one.**
  Adapter selection is a search: preferred, then an explicit software
  fallback, so a host that enumerates an adapter it cannot open still gets a
  device.
- **A value-taking flag with its value missing** (`--connect`, `--attach-pid`,
  `--token`) quietly started the demo server. It is an error.
- **`--list` poisoned the `--attach` that followed it** on Windows. The pipe
  server recycles an instance a client opened and dropped before it could be
  connected, and `connect` waits out `ERROR_PIPE_BUSY`.
- **A stale descriptor, left by an app that exited without unwinding, was
  offered as live.** Descriptors are probed and pruned; one that probes `Busy`
  is kept.
- **A descriptor outlived a failed bridge start**, handing `--attach-pid` an
  endpoint that answers nobody. It is retracted if the accept thread fails to
  spawn.

#### Widgets

- **A text field that does not hold focus no longer paints its selection**, in
  `TextInput`, `PasswordField`, `SpinBox`, `SearchField`, `DateEdit` /
  `TimeEdit` / `DateTimeEdit`, `HexColorInput` and `FilePickerField`.
  **Behaviour change**, visual only: the selection state is unchanged, a field
  that keeps focus while its window goes inactive still dims rather than hides,
  and `RichTextEditor`, `CodeEditor` and `LogView` are unaffected.
- **Enter in a `MessageBox` answers for the focused button**, and falls back to
  `default_button` only when the focus is not on one of the box's own buttons.
  The shortcut carrying Enter is widget-scoped, so it was matched before the
  focused button was offered the key, and the default answered for a button the
  user had deliberately tabbed to. **Behaviour change**, and the case it was
  destructive in is the ordinary one: a `YesNo` confirmation defaults to No, so
  standing on Yes and pressing Enter closed the dialog reporting No, and the
  caller saw a user who had declined. Escape is unchanged, Space was always the
  focused button, and a box whose focus sits on its checkbox still gets the
  default.

### Security

All of these concern the automation bridge, which is debug-only.

- The Windows named pipe carries an **owner-only DACL** built from the process
  token's SID. The default descriptor grants read access to Everyone and to the
  anonymous account. `PIPE_REJECT_REMOTE_CLIENTS` is set as a second layer.
- The Unix socket is `0600` in a `0700` per-process directory, created before
  the descriptor is published.
- The endpoint descriptor, which carries the token, is created with
  `create_new` and its mode in the same `open` rather than written and then
  `chmod`ed, so it also refuses a symlink planted at its path.
- An existing runtime directory must be a real directory rather than a symlink,
  and a mode reachable by others is tightened to `0700`. The documented
  fallback for a Unix with neither `$XDG_RUNTIME_DIR` nor `$TMPDIR` is the
  shared `/tmp`.
- The token handshake carries an end-to-end deadline, so a peer dripping one
  byte per timeout cannot hold the single connection slot.

### Known limitations

- A selected *row* still reports no `IsSelected` on Windows: `Role::Row` is
  absent from `accesskit_windows`' selection-item list, which carries its own
  `// TODO: tables (#29)`.
- `Role::TreeGrid` maps to `NSAccessibilityTableRole`, so a `TreeTableView`
  reads flat under VoiceOver where a `TreeView` does not.
- Keyboard access to the column header — sort, resize, reorder — is missing,
  and remains a WCAG 2.1.1 / 2.5.7 exposure.


## [0.9.2] - 2026-09-03

Accessibility. Every entry changes what a screen reader says, and four of them
fix something that reached no assistive client at all.

### Added

- `TextInput::field_id()`, the id of the inner node that actually takes focus,
  so a form can send focus to the field a validator refused and a modal can
  pick one field out of several. Take the slot before `ctx.add`, read it after,
  as with `caret_setter` and `handle`. `TextInput` also answers
  `initial_focus_hint` with that field now.
- `WindowConfig::app_id`, the identity a desktop matches a window against.
  Defaults to `None`, which leaves winit's behaviour unchanged; set it to the
  basename of the installed desktop entry. One call covers Wayland and X11.
  Windows and macOS ignore it.

### Fixed

- **A labelled `TextInput` was nameless to every screen reader**, on every
  platform, always: the name was written to the composite's own
  `Role::GenericContainer` node, which `accesskit_consumer`'s filter excludes
  unconditionally. It goes on the inner field now, and is locale-reactive where
  the old snapshot was not. `TimeEdit`, `HexColorInput`, `FilePickerField`,
  `SearchField` and `FontPicker` forward into the same call and are fixed with
  it; an explicit `access_label` still wins. **Behaviour change** in what a
  screen reader says.
- **`PreviewCanvas` and the inspector body named a `GenericContainer`** too.
  Both are `Role::Group` now.
- **Nothing focusable advertised `Action::Focus`**, so assistive technology
  could not put focus in a `ListView`, `TreeView`, `TableView`,
  `TreeTableView`, `GridView`, `MenuList` or `OverlayTrigger` — each of which
  is the only focusable node in its subtree. It is derived from the arena's
  focusable flag now rather than left to each widget;
  `access_remove_action(Action::Focus)` still takes it away. **Behaviour
  change:** every focusable node carries the action.
- **No data view told a screen reader which row was current.** Arrowing through
  a `ListView` was silent to NVDA. All five views nominate the current row as
  their `active_descendant`, gated on the view holding focus, and reveal it
  when they take focus; `ListView` also reveals on a selection change.
  `TableView`'s reveal is vertical only. **Behaviour change.**
- **A selection move replaced every realized row node**, resetting the scroll
  offset and the keyboard anchor and handing the accessibility tree fresh node
  ids on every keystroke. `ListItemWrapper` is the rebuild boundary now, so
  only the two rows whose state flipped are rebuilt.
- **A multi-select `ListView`, `TreeView` or `TreeTableView` reported that it
  could hold one row.** `multiselectable` is published, gated on the selection
  mode — `accesskit_windows` picks `ElementAddedToSelection` over
  `ElementSelected` from that property, so publishing it unconditionally would
  raise the wrong event. `TableView` is left out: `Role::Table` exposes no
  selection for the property to describe.

### Known limitations

- `TreeView` publishes no `size_of_set`. A flattened tree cannot express "the
  2nd of 5 siblings" from one container value; that needs a `Role::Group` per
  expanded branch, which changes the tree shape for every tree widget.


## [0.9.1] - 2026-09-02

Accessibility again, plus the documentation sweep that followed it and four
independent fixes. Two of the accessibility entries change what every existing
application announces.

### Added

- **A context menu can be opened from the keyboard**, on every widget that
  already has a `.context_menu(..)`: the dedicated Menu key (`Key::ContextMenu`,
  new), Shift+F10, and Ctrl+Shift+M on macOS, where neither of the other two
  exists. Modifiers are matched exactly and the chords sit below shortcut
  resolution, so an application that binds Shift+F10 keeps it. **Breaking:**
  `Key` is not `#[non_exhaustive]`, so the new variant breaks an exhaustive
  match, and a settings file containing it does not load on an older build.
- `Widget::context_menu_key_target`, default `None`, so a focusable container
  can nominate a descendant as the menu target. All five data views implement
  it against their keyboard cursor, falling back to the first selected row; the
  menu is anchored at the target's own bounds.
- `ctx.announce(msg)` and `ctx.announce_with(msg, Politeness)`, on
  `EventContext`, `BuildContext` and `WidgetTree`: an announcement that reaches
  a screen reader on all three platforms, including repeats of the same
  message. Takes `impl Into<String>`, so `tr!(..)` works directly. Messages
  queue rather than replace one another. Do not pair it with a `show_toast()`
  on the same path — a toast is already a live region, and the user hears
  everything twice.
- `WidgetTree::accessibility_tree_snapshot`, which builds a `TreeUpdate` for
  inspection without caching it, bumping the AT version, recording
  announcements or advancing the live regions.
- `WeakEditorHandle` and `EditorHandle::downgrade`, for a rich-text handler that
  must not keep its own editor alive. Handlers stored on the editor's state
  (`on_image_activated`, `on_link_activated`, `on_change`, `on_text_inserted`,
  the image resolver and the rest) make the state own itself if they capture a
  strong handle, and nothing afterwards can break the ring.
- crates.io metadata on every crate: `homepage`, `documentation`, `keywords`
  and `categories`, so each links to its own docs.rs page. The workspace root
  gained `homepage = "https://teksilo.rs"` and the canonical `repository` URL.

### Changed

- `text-document` 1.12 and `text-typeset` 1.10.
- **A failed `assert_node` is a failure on every automation transport.** It
  returns `AutomationReply::Err { code: "ASSERTION_FAILED" }` rather than
  `Ok(AssertionResult { passed: false })`, so `isError` falls out for the MCP
  server and the socket bridge alike. A property assertion against a node that
  is not in the tree is `NOT_FOUND`; `kind: "exists"` against a missing node
  stays `ASSERTION_FAILED`. **Breaking** for callers that matched on `Ok` and
  read `.passed`.
- **`MessageBoxButtons::YesNo` and `YesNoCancel` default to No**, which also
  takes initial focus. Enter on a confirmation dialog declines. `Ok`,
  `OkCancel`, `SaveDiscardCancel` and `RetryIgnoreAbort` are unchanged. Put Yes
  back with `.default_button(StandardButton::Yes)` where the question is safe.
  **Behaviour change.**
- **A font face is registered without copying its bytes.** `FontFaceSpec::data`
  is `text_typeset::SharedFontData` (`Arc<dyn AsRef<[u8]> + Sync + Send>`,
  re-exported from `teksilo-text`), and `VecFontRegistrar` uses
  `register_font_shared`. A `&'static [u8]` now shares rodata instead of being
  copied twice and held for the life of the process. **Breaking** for code that
  names the field's type; `Arc<Vec<u8>>` still coerces. Worth 12.8 MB of
  baseline in a downstream application bundling eight serifs.

### Fixed

- **No collection ever announced "of N", and `ListView` announced nothing at
  all.** AccessKit resolves `size_of_set` by walking *up* from an item, unlike
  ARIA's per-item `aria-setsize`, and all fifteen of Teksilo's writes were on
  items, so the property was inert everywhere. The count now sits on the
  container in `ListView`, the tab bar, the combo panel, `SearchField`'s
  suggestions, the code editor's completion popup, `GridView`, `ColumnFlow`,
  `SegmentedControl`, `RadioTileGroup`, the stepper and the docking rail.
  `ListView` rows publish their position in the **model**, not in the realized
  window. `TreeView` and `TreeTableView` drop a write no adapter read. See
  0.9.2's known limitation for what a flattened tree still cannot say.
  **Behaviour change.**
- **Every position, row, column and level was one too high.** AccessKit's
  `position_in_set`, `row_index`, `column_index` and `level` are 0-based where
  their ARIA counterparts are 1-based, and both consuming adapters add the 1
  back, so the first tab of five announced as "tab 2" and a root tree item as
  "level 2", with no way to say "level 1" at all. macOS reads none of the four,
  which is why it went unseen there. The public API stays 1-based and the
  conversion happens once at the `AccessNodeBuilder` boundary; a new
  `set_level` joins the other three, and the heading clamp no longer makes
  `<h1>` unreachable. `aria_ordinal_conventions.rs` fails the build if a widget
  reaches past the typed setter. **Behaviour change** across all fourteen
  affected widgets; remove any hand-compensation for the old behaviour.
- **53 places where the documentation contradicted the code.** Among them:
  `teksilo = "0.7"` in three places in the app guide; a claim that builder-call
  order is irrelevant, when `install_toast_default()` panics unless
  `.application(..)` came first; a minimal-build recipe that dropped the `i18n`
  feature and so did not compile; `Expand::flex`'s own doc writing `flex(0)`
  against an `f32`; and 19 uses of non-existent `ButtonVariant` names in the
  teksu spec. Four dead fields went with them, the two competing `typos`
  configurations became one at the repository root, and the widget catalog is
  regenerated.
- **A keystroke recoloured the whole rich-text document.** A block with no
  overlay reported a miss rather than the no-op it had performed, and the frame
  loop treats a miss as grounds to fall back to `flow_snapshot()`.
- **A closed window never released its event subscriptions**, holding whatever
  its closures captured for the life of the process:
  `TreeAppContext::subscription_callbacks` is shared by every window and was
  purged per-widget only. Plain entries carry a window id now.
- **A long category label starved a chart's plot to nothing.** A tilted x-axis
  band was subtracted from the chart height with no floor, and both `BarChart`
  and `LineChart` return early on a zero-height plot — no bars, no grid, no
  axis, no diagnostic. The band is capped at half the chart height and labels
  ellipsize into the width they are granted.
- **Observing a derived signal panicked.** `Signal::observe` rejected anything
  built with `map`, `zip`, `and` or `not`, so the macOS native-menu bridge
  aborted the process before the first window was drawn, through an
  Objective-C frame it could not unwind. Derived signals subscribe on every
  upstream root now. `flat_map` still reports `ReadOnly`, having no fixed root.
- **The X11 drag-and-drop teardown is confirmed by the server**, so `XdndProxy`
  cannot outlive the window it points at and leave the property aimed at a
  destroyed window. 6 failures in 300 runs before, 0 in 600 after.
- **Publish order ignored dev-dependencies and the umbrella crate**, which is
  what failed three crates mid-release for 0.9.0.


## [0.9.0] - 2026-08-31

### Added

- **The framework's own strings speak twenty-one more languages.** ar-SA,
  cs-CZ, da-DK, de-DE, el-GR, es-ES, fi-FI, he-IL, hu-HU, it-IT, ja-JP, ko-KR,
  nb-NO, nl-NL, pl-PL, pt-PT, ro-RO, ru-RU, sv-SE, tr-TR and uk-UA join English
  and French in `framework_locales()`, covering all 308 user-facing strings.
  Each carries its real CLDR cardinal plural categories rather than a copy of
  English's one/other shape.
- `runtime_override` takes a directory as well as a single `.ftl` file, and the
  watcher follows every resource under it. Reloading one file used to replace
  the locale's whole bundle, dropping every key its siblings defined.
- **A number can be read back in the locale it was written in.**
  `NumberSymbols` recovers a locale's separators, signs and digits from ICU's
  own formatted output, so the display and parse directions cannot drift, and
  it works in strings rather than `f64`, so an `i64` past 2^53 keeps its
  precision. `SpinBox` gained `.localized(bool)`, **default on**, and
  `.use_grouping(bool)`, default off as in Qt. **Behaviour change:** a French
  user sees and types `12,5`.
- **The widget catalog renders its own pictures.** `teksilo-widgets-previewer
  --export-docs` renders every registered widget through the production wgpu
  renderer into `docs/widgets/img/`, headless and with no display server; 95 of
  the 136 pages open with a preview. The images are committed rather than built
  in CI, which has no GPU adapter.
- `WidgetTree::text_surfaces()`, `focused_text_surface()` and
  `focused_is_text_surface()`, so an application owning a global `Ctrl+Z` can
  route it to whatever is being edited, including surfaces it did not build.
- **Cell editing with click triggers and persistent focus.** A per-column
  `EditTriggers` bitmask chooses single or double click,
  `on_cell_edit_dismissed` ends an edit on a click away, and
  `BuildContext::focus_into` keeps keyboard focus in a tree-table editor across
  rebuilds. Double-click-to-edit no longer
  costs the row selection on the first click.
- `Button::icon_keeps_color`, for a glyph whose colour is the information — a
  filter chip's tag colour, a legend swatch, a status disc. The same opt-out
  `MenuItem` already had.

### Changed

- **Overlay and tooltip bodies are built on first use** rather than on every
  rebuild of their owner. New `BuildContext::add_deferred`, `add_deferred_boxed`
  and `DeferredSubtree`. The old `ctx.add` + `set_dormant` pair paid a full
  `build()` for content the user may never open, which is invisible in a dialog
  and dominant in a virtualized collection.

### Fixed

- **A language switch did not reach the date and time fields.** Each derives its
  convention — pattern, first day of week, 12- vs 24-hour clock — once in
  `build()`, and `set_locale` deliberately does not rebuild.
- **Escape reached neither past a tooltip nor out of a text field**, two
  independent swallows: the dismissal path treats a hover-shown tooltip as a
  dismissal target and returns, and a focused `TextInputField` had no `Escape`
  arm, so it fell into the printable-character branch. Every dialog whose
  cancel sits outside a field had the same hole.
- **A menu row activated by keyboard panicked on `open_window`** while the same
  row opened its window fine by mouse: the synthetic click was drained through
  `WidgetTree::click`, the test entry point, which installs `NoopWindowOps`.
- **A drop into a folder looked like a drop after it.** Three verdicts shared
  one visual: the `Into` box was painted at the row's exact bounds, so its
  edges occupied the pixels the `Before` and `After` lines use.
- **The four Level-A findings of the 2026-08-28 internal accessibility
  assessment**, including a keyboard trap in `Terminal` (WCAG 2.1.2), which
  answered `Handled` to every `KeyDown` so no chord could leave the widget. The
  assessment document was re-checked against source and reframed as an internal
  engineering assessment — not an ACR, not a VPAT, not third-party verified —
  with a section stating what was not done.


## [0.8.0] - 2026-08-27

Two complete design languages, and the macOS accelerator convention that
building them exposed.

### Added

- **The macOS (Aqua / Dark Aqua) preset**, replacing a stub: opt in with the
  umbrella crate's `theme-macos` feature, reach it as
  `teksilo::prelude::macos::{light, dark}`. 28 style slots, eight of them real
  `impl FooStyle` blocks (push-button bezel, accent focus ring, `NSSwitch`,
  14 dp checkbox and radio, field focus halo, slider knob, menu row, list-row
  selection capsule). The full AppKit vocabulary — four label grades, two
  selection families, the bezel description, the eight System Settings accents
  — is on the `MacOsPalette` extension. `light_with_accent(Color)` /
  `dark_with_accent(Color)` and the `SystemAccent` enum rebuild the accent
  family; `linkColor` deliberately does not follow. Apple publishes almost none
  of this, so every literal is tagged at its definition `[HIG]`, `[measured]`
  or `[derived]`, and four deviations from Apple's own numbers carry the
  contrast measurement that forced each.
  **Known limitations:** the OS accent is not read (the platform layer returns
  only light/dark on macOS); vibrancy uses each material's opaque fallback; the
  `TableView` / `GridView` selection band is an accent wash rather than the
  capsule; San Francisco is named under the optional `system-fonts` feature,
  not bundled.
- **The Fluent (Windows 11 / WinUI 3) preset**, likewise, behind
  `theme-fluent`, as `teksilo::prelude::fluent::{light, dark}`. Every colour is
  transcribed from WinUI's own `Common_themeresources_any.xaml` in `#AARRGGBB`
  notation, so the file diffs against the theme dictionary line by line. 25
  style slots, eight real `impl FooStyle` blocks (button elevation edge,
  two-tone focus ring, `ToggleSwitch`, filled unchecked checkbox and radio,
  field accent underline, two-circle slider thumb, neutral menu hover, list-row
  selection pill). `FluentPalette` carries the graded control fills, on-accent
  strokes and system fills `ColorTokens` has no slot for.
  `light_with_accent(Color)` / `dark_with_accent(Color)` rebuild the accent
  family, leaving every neutral untouched.
  **Known limitations:** Mica and Acrylic use the opaque fallback WinUI itself
  falls back to; Segoe UI Variable is named under `system-fonts`, not bundled.
- `StandardMenu::settings_intent("app.settings")`, with `.settings(label)` for
  the text: a **Settings…** row in the macOS App menu, under About, on ⌘, —
  placement an ordinary `MenuEntry` cannot reach. Unlike Quit there is no system
  fallback, so leaving it unset omits the row rather than rendering a dead one.
- The automation `scroll` op takes `ctrl` / `shift` / `alt` / `meta` beside
  `dx` / `dy`, mirroring `inject_key`. All default false. A probe could
  previously describe Ctrl+wheel but not perform it.
- `StandardItemStyle::selected_label_role` and
  `MenuItemStyle::highlighted_label_role`, both defaulting to `None`, so a
  design language with a solid selection fill can recolour the text on top of
  it. A row builds its label before any `make_body` runs, so it could not.
- `TwistArrow::color(impl Into<ColorProp>)`, taking any colour, role or signal.

### Changed

- **A declared `Ctrl` shortcut fires on ⌘ on macOS.** `KeyStroke::ctrl(Key::F)`
  is read as the platform's primary accelerator, the convention Qt spells
  `Qt::CTRL` and the one the native menu bar always applied when building key
  equivalents. **Behaviour change on macOS:** the chord no longer fires on
  physical ⌃. Opt a chord out with `ShortcutBuilder::literal_modifiers()` —
  Ctrl+Tab, and anything whose ⌘ form the system takes. User overrides stay
  literal, so ⌃F is still bindable. New API: `Modifiers::COMMAND` /
  `Modifiers::command()`, `KeyStroke::command()` / `command_shift()` /
  `with_command_convention()`, `Shortcut::declared_keystrokes()`,
  `ShortcutBuilder::literal_modifiers()`.
- The widget catalog's `ThemeSwitcher` and `--theme` offer macOS and Fluent
  (`macos-light`, `macos-dark`, `fluent-light`, `fluent-dark`). Theme
  restore-on-launch moved to a named `theme_from_id`, and the example gained
  its first tests, including a persist/restore round trip over every offered
  preset.

### Fixed

- **Caret motion follows the platform's own layout.** macOS spreads word, line
  edge and document across three modifiers on the arrows — `⌥←/→`, `⌘←/→`,
  `⌘↑/↓`, with `⌥↑/↓` for paragraph — where Windows and Linux use `Ctrl+←/→`
  and bare `Home`/`End`. Every text surface read a single accelerator flag, so
  `⌘←` jumped a word, `⌘↑` moved one line, and `⌥←` did nothing. The chords go
  through the new `common::text_nav`, fixing `RichTextEditor`, `CodeEditor` and
  everything on `TextInputField`; word-delete moves with them. `Alt+↑/↓` keeps
  move-line in the code editor on every platform. `LogView` is untouched, having
  no caret. `⌘⌫` (delete-to-line-start) is not implemented and falls through to
  a single-character delete.
- **Accelerator chords across the widget catalog**: select-all, the
  discontiguous-selection click, the marquee's additive modifier and Ctrl+Home /
  End all tested physical Control, so on macOS ⌘A did not select all while
  ⌃-click — the secondary click there — did extend a selection. The five data
  views, `Calendar` and the colour picker's swatch grid test
  `Modifiers::command()` now. The text surfaces tested `ctrl() || super_key()`,
  which also made the Win key act as Ctrl on Windows and Linux. The
  Explorer-style cursor pair, Ctrl+Tab, Ctrl+Space and the terminal's control
  codes deliberately stay literal and say so at their sites. Built-in
  context-menu labels built their accelerator text from hard-coded `Ctrl`, so a
  right-click read "Copy ⌃C" while the menu bar showed ⌘C.
- **A tree row's chevron ignored its row's colour.** `TwistArrow` painted a
  hardcoded `TextRole::Secondary`, which under a style that flips a selected
  row's label left a grey smudge on an accent capsule at roughly 2.5:1, under
  WCAG SC 1.4.11's 3:1 floor. `StandardTreeItem` hands it the label's own role.
- **The `SpinBox` mouse wheel was inverted** against `QAbstractSpinBox`,
  `GtkSpinButton`, WinUI's `NumberBox` and its own ArrowDown key: `ScrollDelta`
  is already negated so positive y increases a scroll *offset*, and `SpinBox`
  is the one consumer mapping a notch to a value. The wheel path had no test
  coverage; it has three.


## [0.7.0] - 2026-08-08

### Added

- **`DockAction`**, a dockless command button in the docking activity rail,
  added with `DockRail::action(..)` and rendered by the framework so it matches
  a real activity item. Deliberately more restricted than an activity: never
  draggable, never hidable, never overflow-parked.
  `DockActionPlacement::{Start, End, Pinned}` picks the cluster, `Pinned`
  sitting past the spacer at the rail's far edge. `DockActionId::named("…")` is
  a `const fn`, so ids are module-scope constants and stay stable across runs
  for the accessibility tree and the automation bridge.
  `DockAction::toggled(signal)` is **reflect-only** — the rail never writes it,
  so a derived signal is safe. Nothing about an action is persisted:
  `DockLayoutState` is unchanged and needs no migration, and `DockPolicy` has
  nothing to gate.
- `DockRail::leading_slot` / `trailing_slot`, reaching a side that shows its tab
  strip rather than its rail, where `top_slot` / `bottom_slot` could not. They
  are composed with the "hidden activities" hamburger `DockSidePanel` already
  spends the trailing slot on, so declaring one cannot silently drop the other,
  and a side with zero docks renders them rather than returning early. Their
  visibility contract is weaker than the rail slots' and documented as such:
  they live inside the collapsing content region and disappear with it.
- `ListModel::reconcile_by_key(new_items, key_fn)`, which diffs the live model
  against a new authoritative `Vec<T>` and emits minimal granular `DataChange`s
  — coalesced removes and inserts, single-row moves only where something is out
  of place — and **never** a blanket `Reset`, which would clear the user's
  positional selection on every peer write. This is what
  `PersistedListModel<T>`'s live-reload path is built on.
- `adjust_single_index_for_change` in `data_change.rs`, used by `ListView`'s
  focused-index tracking so a single-selection widget keeps its focus on the
  right row across a reload-driven reconciliation.

### Changed

- **`teksilo-settings` is cross-process safe by default**, not by opt-in.
  `SettingsStore`, `SettingsFile<T>` and `PersistedListModel<T>` — and
  `MruList<T>` and `WindowStateService` on top of them — merge every write
  against the document read fresh under an exclusive lock, and a peer process's
  write arrives live through the `Signal` or `ListModel` a caller is already
  bound to, via a new `notify`-based watcher. Nothing to remember to call,
  unlike `QSettings::sync()`. See [docs/settings.md](docs/settings.md).
  **Breaking:**
  - `SettingsFile::load_shared` is removed; `load` / `load_strict` are
    unconditionally correct, and dropped their now-meaningless `delay`
    parameter.
  - `MruList::toggle_pin` is removed, replaced by `set_pinned(key, bool)`. A
    toggle's effect depends on the state at the moment it runs, which a
    replayable write cannot assume.
  - `PersistedTreeModel<T>` and its `collection::tree` module are removed. They
    had no consumers and carried the whole-snapshot clobber the rest of the
    crate was being hardened against. Reintroduce it ops-based if a real
    consumer appears; do not resurrect the deleted version.
  - `Keyed` is a new trait (`type Key`, `fn key(&self) -> Self::Key`) that
    `MruEntry` now requires alongside its pin and touch methods. An existing
    `impl MruEntry` needs a companion `impl Keyed`.

  `SettingsFile::mutate`, every `SettingsStore` signal, and `MruList::add` /
  `touch` / `remove` / `clear` keep their call-site shape.
- `NotificationArchiveModel::remove(index: usize)` is now
  `remove_by_id(id: u64)`. **Breaking:** an index names a position a peer's
  insert, or this crate's own debounce, can invalidate before the removal
  runs.

### Fixed

- **The docking activity rail was an invalid ARIA `tablist`.**
  `DockActivityBar` set `Role::TabList` on its whole root, so the slots, the
  overflow trigger and the action clusters were non-`Role::Tab` children of a
  tablist, which the ARIA Tabs pattern forbids and real screen readers navigate
  poorly. The role sits on a `DockRailTabList` wrapping only the items now, with
  everything else a sibling under a property-free root the AT pass prunes.
  Restricting the AT children instead would have deleted those controls from the
  tree, trading a spec violation for a WCAG 2.1.1 failure. No app-facing API
  changed.


## Earlier history

Entries before this file was introduced are not backfilled; see `git log`
for the full history.

[Unreleased]: https://github.com/FernTech-EU/teksilo/compare/v0.9.3...HEAD
[0.9.3]: https://github.com/FernTech-EU/teksilo/compare/v0.9.2...v0.9.3
[0.9.2]: https://github.com/FernTech-EU/teksilo/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/FernTech-EU/teksilo/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/FernTech-EU/teksilo/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/FernTech-EU/teksilo/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/FernTech-EU/teksilo/compare/v0.6.2...v0.7.0
