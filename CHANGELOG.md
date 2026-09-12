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

## [0.9.5] - 2026-09-11

Three main strands: text ranges on every visible label, one chord table for
the bounded-scalar controls, and a submenu that survives the diagonal to
reach it. Plus `stretch_last_column` on the tables, an assistive-technology
whole-value write that commits, and a `FormLayout` that keeps its rows
across a rebuild. One breaking change: `TextInputField::on_access_set_value`
now returns whether the host accepted the string.

### Added

#### Touch, pen and density

The framework's input model was a mouse: one pointer, always hovering, always
precise, always present. It is now a pointer model, and a mouse behaves exactly as
it always has.

- **Touch input, end to end.** A touchscreen's contacts reach widgets as real
  pointers with their own identities, and a finger scrolls, selects, reorders,
  edits text, pinches, drags to and from the OS, and reaches a 24 dp target. Each
  contact gets a fresh identity per press, so an OS that reuses slot numbers cannot
  make two gestures look like one, and up to a documented cap of simultaneous
  contacts are tracked at once.
- **Pen and stylus.** Pressure, tilt, barrel rotation, the eraser end, the barrel
  button, and proximity as a first-class state — a stylus hovers before it touches.
  Wayland through `zwp_tablet_v2`, Windows through a `WM_POINTER*` window subclass;
  no winit upgrade required.
- **Trackpad gestures.** Pinch, rotate and two-finger scroll reach the widget tree.
- **`TargetDensity` — Compact, Comfortable and Touch.** One knob resizes every
  interactive target, gap and padding in the application: 24 / 32 / 44 dp targets,
  6 / 10 / 16 dp grab handles, spacing ×1.00 / 1.15 / 1.30. Compact is the default
  and is today's layout unchanged. Set it with `Theme::with_density` or
  `WidgetTree::set_input_density`; the shipped Material 3, Fluent and macOS presets
  project their own dimensions onto the ladder rather than inheriting Teksilo's.
- **A 24 dp conformance floor at every density, never scaled** — WCAG 2.2 SC 2.5.8
  (level AA). A build-breaking test measures every named widget fixture against it
  at all three densities.
- **Three hit mechanisms with disjoint jobs**, so a small control is reachable
  without being redrawn: `Widget::hit_outset` widens a target for one pointer kind
  only, `Widget::target_regions` lets a self-painting widget say where its parts
  are, and a miss-only slop pass re-attributes a near miss for a coarse pointer.
  None of them moves a painted pixel.
- **Kinetic scrolling.** A fling coasts and settles on the platform's own curve —
  Android's `OverScroller` or iOS's bouncing simulation, chosen per host by default
  — with rubber-band overscroll available per surface. Nine scrollable surfaces
  adopt it from one implementation.
- **Touch text editing** in every editing surface: the caret lands on the release,
  a hold selects the word under the finger, two draggable handles adjust the range,
  a magnifier shows the text under the handle, and a selection toolbar offers the
  clipboard. `RichTextEditor`, `CodeEditor`, `PlainTextEditor`, `LogView`,
  `TextInput`, `PasswordField`, `SearchField`, `SpinBox` and the date/time family.
- **A hold is a route.** Where a widget installs no long-press handler of its own,
  the tree resolves a hold to the node's context menu, else its tooltip, else
  nothing — so a finger reaches both affordances that were previously mouse-only.
  `LongPressRole` overrides the choice per subtree.
- **`touch_action` and `pan_claim` node declarations**, the CSS `touch-action`
  model: a subtree can forbid panning or pinching over itself, and a scrollable
  declares which axes it claims. A press freezes the effective value, so nothing
  a widget does mid-gesture can change what that gesture was allowed to become.
- **A runtime kill switch.** With `InputTokens::touch_enabled` off, the platform
  translator drops touch input and the router installs no touch-only recognizers —
  a mouse-only fallback with no rebuild of the binary.
- **Input tracing.** `TEKSILO_TRACE_INPUT=samples|gestures|all` prints every
  sample, every arbitration decision and every cancellation, guarded so that a
  normal run formats nothing.
- **`examples/touch_playground`** — a live pointer inspector (identity, kind,
  primary flag, pressure, tilt, twist, contact patch, speed, the frozen touch
  action, the press and the capture), five arbitration scenarios each naming which
  contender won the press, a density toggle, an editable kinetic-constants panel,
  and a touch-text surface.
- **A Touch tab in `examples/widget_catalog`**, and `--density compact|comfortable|touch`
  on the catalog itself.
- **[docs/touch-verification.md](docs/touch-verification.md)** — the hardware
  procedure, and [its sign-off sheet](docs/touch-verification-signoff.md).

#### Bounded-scalar controls

See [docs/range-keyboard.md](docs/range-keyboard.md) for the full chord table.

- `common::range_nav`: the arrow, page and edge chords of a control that holds
  one bounded number, as pure functions over
  `(key, modifiers, kind, axis, direction)`. `Slider`, `SpinBox`, `ScrollBar`,
  the colour picker's hue and alpha strips, the `Splitter` handle and the dock
  resize handle each hand-rolled the same eight-key match and gave four
  different answers for it. Like `list_nav`, it takes no platform convention:
  Qt, GTK4, the Win32 trackbar and `<input type=range>` bind these keys
  identically on all three desktops.
- `PageUp` / `PageDown` in `Slider`, with `Slider::page_step` to size the jump.
  Unset it defaults to ten times the effective `step`, so with the default step
  of 1 % of the range a page is 10 % of it — WebKit's and Blink's rule for
  `<input type=range>`, `QAbstractSlider::pageStep`, `GtkScale`'s page
  increment, and the same `10 x` rule `SpinBox::page_step` already used. The
  slider was the only one of Teksilo's bounded-scalar widgets with no coarse
  step at all.
- `Slider` publishes `numeric_value_jump` and services `Action::SetValue`,
  taking either an `ActionData::NumericValue` or a numeric string and snapping
  to `step` exactly as a drag does. AT-SPI publishes the `Value` interface off
  `numeric_value` alone, so Orca's `Value.SetCurrentValue` already reached the
  widget and was dropped; macOS gates `setAccessibilityValue:` settability on
  the advertisement, which the node now carries.

#### Widgets

- **`Alt+ArrowDown` opens the popup and `Alt+ArrowUp` closes it** on
  `ComboBox`, `DateEdit` and every `PopoverButton` / `PopoverIconButton` — so
  `ColorEdit`, whose documentation has promised the chord since it was written.
  The Win32 / WinForms / WPF drop-down chord, the Win32 `DateTimePicker` chord,
  and the W3C ARIA combobox pattern: "displays the popup without moving focus",
  which is why the modified form exists beside a bare `ArrowDown` that both
  opens and advances.
- `TextInputField::on_access_set_value` (forwarded by `TextInput`): handle an
  assistive technology's whole-value write, given the string it set. Unset for
  a field whose bound `Signal<String>` **is** the value — the write has already
  landed and there is nothing to derive. `SpinBox` and the four date and time
  editors install one, because their text is only a projection of a typed
  value. The string is handed over rather than read back from the signal
  because the field defers its document→signal sync to the next frame tick, so
  a host reading the signal there would parse the text from *before* the edit.
- **`F4` toggles the popup** on the drop-down *fields* — `ComboBox` and
  `DateEdit` — the Win32 / Qt / WPF chord. Deliberately not on `PopoverWidget`,
  which also backs toolbar chevrons and menu buttons and carries no such
  convention. An app that registers `F4` as a `Shortcut` keeps it: shortcuts
  resolve before the focused widget sees the key, and `Alt+F4` is unaffected.

#### Accessibility

- **Every visible label that owns its own accessible name is reviewable by
  character, word and line**, routable on a braille display and trackable by a
  screen magnifier. `TextWidget` — the building block behind every label in the
  framework — now carries AccessKit text runs with real per-character extents,
  so a screen reader's review cursor can walk a paragraph, a braille cell can
  be routed to the word under it, and `AXBoundsForRange` / UIA's `TextPattern`
  / AT-SPI's `GetCharacterExtents` all answer. Before, a label was one
  unreviewable chunk; only the editors and the terminal exposed ranges, and
  even those exposed no geometry.
- `TextWidget::geometry_handle`, for a control that owns its name but paints
  its text through a hidden label. `Badge`, `GroupHeader` and the scene's
  `TextItem` use it and are now reviewable too.
- `teksilo_core::accessibility::text_runs`: the shared emitter every text
  surface goes through, and `teksilo_core::accessibility::audit`, which reports
  labels that repeat an ancestor's name, labels with no text ranges, and runs
  that disagree with the node they hang off.
- `DocumentFlow::block_line_geometry` and the `*_with_geometry` layout methods
  in text-typeset 1.11, which is where the per-character extents come from.

#### Data views

- `TableView::stretch_last_column` / `TreeTableView::stretch_last_column`:
  the last column in display order takes the width the other columns leave
  (Qt's `stretchLastSection`). Positional — it follows a reorder — and the
  stretched column has no grip of its own.

#### Core

- `EventContext::arm_overlay_safe_region` / `overlay_safe_region_armed`: an
  open overlay can claim a "safe triangle" from the point the pointer left its
  anchor to its own near edge. While the pointer is inside it, the overlay's
  pointer-leave grace is held off; leaving it starts that grace and coming back
  cancels it. Any widget whose hover would tear the overlay down asks whether a
  traversal is under way at all and stands aside for as long as one is, leaving
  the dismissal to that one re-evaluated grace. Bounded to 600 ms so it stays a
  travel allowance. Used by cascading submenus.
- `EventContext::show_overlay_after_replacing_siblings`: like
  `show_overlay_after_with_focus`, but the anchor's sibling overlays are
  dismissed when the overlay actually shows rather than when it was requested
  — the hover-switch that must not fire for a pointer merely passing through.

#### Automation

- `teksilo_automation::client`: the binary name, the matching version and the
  `cargo install` command for `teksilo-automation-mcp`, plus a `$PATH` lookup
  for it. Pure `std`; nothing spawns a process, because the only caller is on an
  app's startup path.
- **An agent can drive touch, a stylus and two contacts.** New operations:
  a whole multi-touch sequence in one call (reporting the arbitration after every
  step), a two-finger pinch, a fling described in simulated time, a long press
  held for the active profile's own threshold, a pointer cancelled the way the
  system cancels one, a query of every live pointer with its capture and
  arbitration state, and a density switch. `inject_pointer`, `inject_key` and
  `type_text` take a `command` modifier beside `ctrl`, because a chord declared
  `Ctrl+S` resolves to ⌘S on macOS.

#### Accessibility

- **The target-size gate measures every shipped preset**, not the default theme
  alone: Int UI, macOS, Fluent and Material 3, each at all three densities, with
  light and dark compared for identical geometry. The fixtures moved into a
  crate the theme crates can reach, so a preset is audited by the same 70
  fixtures the default theme is.

#### Previewer

- **`--export-docs` renders at a chosen density**: `--density=compact,touch`
  writes `docs/widgets/img/<slug>-touch.png` beside the canonical image, and a
  catalog page grows a `## Density` section when one exists. Compact stays
  canonical and keeps every existing filename, so nothing committed moves.
- `teksilo_preview::PreviewPass` — the density a preview renders at, and the
  image-naming rule that follows from it.

#### Documentation

- **`docs/porting-widgets-to-the-pointer-model.md`** — the contract for moving a
  widget onto the pointer model, as numbered clauses, each stating the rule, how
  to comply, and what checks it.
- Documented constants are asserted against the pages that document them. The
  density ladder, all three gesture profiles and the kinetic constants are read
  back from their own tables by a test, so a number lives in one place.

### Changed

#### Core

**The pointer-model behaviour changes.** Each is a token or a knob an application
can override, and none of them changes what a mouse does.

- **A mouse drag never pans.** Dragging the content of a scrollable surface
  scrolls it only for a touch or a stylus; a mouse scrolls with its wheel, exactly
  as before. Widen it per surface with `ScrollHandlingOptions::pan_devices`.
- **A touch or pen drag of an *item* waits for a long press.** Reordering a list
  row, a grid tile, a tab or a table column with a finger means holding it first,
  because the surface underneath has first refusal on a direct pointer's press. A
  mouse still drags from the first few pixels. `DragActivation` on the node decides;
  `Auto` is the default and resolves per pointer kind.
- **A touch or pen press commits on the release.** Selection, activation and caret
  placement happen when the contact lifts, not when it lands — so a press that
  slides off its target commits nothing. A mouse commits on the press, unchanged.
- **A press outside an overlay dismisses it on the release, and the dismissing
  press does not reach what is under it.** Dismissing a menu and activating the
  control beneath it are now two presses, on every pointer kind.
- **`on_hover` never fires for a touch contact,** and neither do `hover_within`
  and the hovered signals. A contact does not hover — that is what a contact is —
  so an affordance revealed only on hover is unreachable with a finger.
  `InputTokens::reveal` promotes those affordances to always-visible at the Touch
  density, and the tree's hold route reaches a tooltip regardless.
- **A pen in proximity hovers, and hover belongs to one pointer.** On a machine
  with both a mouse and a stylus, each one's hover follows its own device instead
  of a single shared "hovered" notion.
- **`advance_time` ticks gesture recognizers.** A test that advances the virtual
  clock now sees a long press ripen, a fling coast and a hold dispatch; the clock
  is one axis for animation, gestures and kinetics rather than three.
- **`WidgetEvent::PointerDown`, `PointerUp`, `PointerMove`, `PointerEnter` and
  `PointerLeave` now carry `pointer: PointerInfo`**, so a handler can tell a
  finger from a stylus from a mouse without reaching for the context.
  `PointerEnter` and `PointerLeave` become struct variants; a pattern that named
  them bare now needs `{ .. }`.
- **`PointerMove` also carries `modifiers: Modifiers`.** A drag reads Shift and
  Ctrl from the move rather than from the press, so a modifier pressed mid-drag
  reaches the widget.
- New constructors keep a mouse-describing call site to one line and default the
  pointer to `PointerInfo::mouse` at the epoch: `WidgetEvent::pointer_move_with`,
  `pointer_enter`, `pointer_leave`, beside the existing `pointer_down`,
  `pointer_up`, `pointer_move`.
- **`WidgetEvent::Scroll::position` and `PointerCancel::position` are renamed
  `window_position`.** Both stay in window-logical coordinates — the router
  routes by the first, and the kinetic tracker behind a pan follows the pointer
  rather than the widget — and they are the only positional fields a handler
  receives that are not localised to it. The frame is now in the name, so a
  reader that needs content coordinates is told to convert at every use.
- **`GestureEvent::PinchChanged` carries deltas, and says so.** `scale` is the
  factor since the *previous* sample and `rotation` the twist since the previous
  sample, in radians; fold each in rather than assigning it.
- **A drag handler is told which device carries the drag at every stage**,
  including the per-layout tick. Those previously reported the mouse, so a finger
  never got the wider auto-scroll band.
- **A coarse pointer's drag preview is placed clear of the contact patch**; a
  mouse's is unchanged.
- **The widget under an inbound OS drag revises the OS accept state**, so the
  cursor no longer promises a drop the target refuses. An OS drag aborted over a
  second window of the same application tears that window's session down, and a
  drag exported from a finger on Wayland uses that finger's press serial.
- **`WindowOps::begin_os_drag` and `ExternalDndGuard::begin_drag` take the
  device's `PointerKind`**, and both gain `set_drop_accepted`.
  `ExternalDragEvent` gains a `Cancelled` variant.
- **`TouchSelection::on_long_press` takes a `pointer: PointerInfo`.** A hold is
  recognised by a timer, so the context it is dispatched under had no device to
  report; the holding contact's identity is now an argument.
- **`WebViewHandle::set_input_passthrough` is a new required trait method**, for
  out-of-tree backends. `WebViewEvent::EngineFocusChanged` is new.
- **`MemoryShared` is `#[non_exhaustive]`** and gained four fields.
- `InspectorState::toggle()` is public. New:
  `teksilo_widgets::drop_target::{band_depth, region_at_floored, region_rect_floored}`,
  `styles::recipe_menu_item_style::menu_item_height`.

#### Widgets

- **Nine surfaces pan under a finger** from one implementation: `ScrollArea`, the
  five data views and the three text surfaces, plus `Terminal` and `SceneView`,
  which claim their own axes.
- **The window chrome answers a finger.** A resize strip stays 6 dp of paint and
  reaches the density's target size for a coarse pointer; a hold on the title bar
  asks the OS for its window menu where the platform has one.
- **A `SplitButton` chevron takes a direct pointer's press up to the conformance
  floor**, borrowing from the action half; a mouse's boundary stays where it is
  painted.
- **A `TableView` header cell's filter zone is the density's target size**, and
  the cell reports its three parts through `Widget::target_regions`.
- **A `DropTarget`'s edge zones — and a docking pane's five — are floored per
  axis and capped at a third of the extent.** Not pointer-kind-gated: the acting
  zone and the painted one must be one rectangle.
- **List and tree row minimum heights, combo-box dropdown rows, calendar
  navigation arrows, menu rows and code-editor completion rows follow the
  density ladder.**
- **A finger's tap on a `ToolBox` header or a `RadioTile` leaves it untinted**
  rather than resting hovered.
- **A toast pauses on a press-and-hold and dismisses on a horizontal swipe** for a
  coarse pointer, because hover-to-pause is unreachable with a finger.
- **Every reorder a drag can do, a keyboard and a menu can do too** — WCAG 2.5.7.
- **`WebView` in the default `Native` input mode** answers a press itself and
  revokes the pointer's interaction, declares no touch default over its rectangle,
  and opts out of hit widening; `Transparent` asks the engine to stop taking
  input. Its subview is mirrored at the intersection with every clipping ancestor,
  hidden when out of view, and stood down while an interactive overlay covers it.
  Engine focus moves the toolkit's focus onto the frame.
- **The debug inspector opens from `InspectorState::toggle()` or a corner-grip
  hold**, its pressed rows take the density's target size, and it has a Pointers
  tab listing every pointer declaration in the application's tree.
- **The widget previewer takes `--density`, switches it live, and its canvas zoom
  works** (pinch and Ctrl-wheel).

#### Terminal

- **A finger pans the scrollback with a kinetic hand-off**, quantised to lines so
  a sub-line sample is banked rather than dropped. Double- and triple-tap select
  the word and the line, a hold opens the terminal's own menu, and selection
  handles snap to cell boundaries.
- **A finger is never reported to the child program as a mouse.** Touch reporting
  is its own policy, separate from the mouse reporting a program requests.

#### Widgets

- `TextInputField::on_access_set_value` (and `TextInput`'s forwarder) now takes
  a callback returning `bool` — whether the host accepted the string — so the
  field can report a refused write to the assistive technology instead of
  claiming success. **Breaking.**
- `ScrollBarStyle::make_body` documents its right-to-left obligation, the way
  `SliderStyle::make_body` does: a horizontal bar's `scroll_ratio == 0.0` is the
  start of the content, which is the right-hand edge there, and the widget
  cannot enforce that a custom style mirrors.

#### Accessibility

- Dialogs and groups are named by their visible title through a `labelled_by`
  relation rather than by a copy of it, so the title stays reviewable in its own
  right. Live regions — `Banner`, `Toast`, `MessageBox` — keep their own name
  instead, because the announcement path reads a node's own value.
- The code editor and log view no longer announce "line 42 of 200". The ordinal
  needed a `Role::Paragraph` node per line, and that node is what dropped every
  text-change event on all three platforms; line position is available through
  line navigation everywhere, and in `CodeGutter`.
- `TextLayout` carries an optional `geometry`, `AccessNodeBuilder::build`
  returns a fourth element (the widget-local rects of any synthetic children),
  and `MockTextBackend` measures characters rather than bytes and treats a
  newline as a hard break.
- A markup label with `TextOverflow::Ellipsis` now measures as one line, which
  is how it was already painted.

#### Automation

- **The bridge announce now says when the MCP client is not installed**, and how
  to get it. An app needs nothing installed to be automatable: the bridge is
  compiled into the debug build and binds its own endpoint, and
  `teksilo-automation-mcp` is only the client an agent drives it through. That
  asymmetry is easy to get wrong from outside, and a harness that got it wrong
  ended up carrying its own copy of where to look, which version to ask for, and
  an account of this crate's protocol history. The absence is now reported where
  the instruction to use the client is given.

### Fixed

#### Bounded-scalar controls

- **`Ctrl` / `Alt` / `Super` chords no longer drive a bounded control.**
  `Ctrl+ArrowUp` stepped a `SpinBox`, `Ctrl+Home` drove a `Slider` to its
  minimum, `Ctrl+PageDown` scrolled a `ScrollBar` and `Ctrl+ArrowRight` resized
  a `Splitter` — each answering the key and reporting it handled, so the chord
  never reached the application's own Shortcut/Action pipeline. They fall
  through now, which is what `QAbstractSpinBox` does. `Shift` is deliberately
  untouched: it is not a distinct chord on any of these controls, and the
  single-line field binds nothing to `Shift+Up`, so rejecting it would only
  leave the chord dead. **Behaviour change.**
- **A `Splitter`'s resize arrows move the divider the way they point**, on
  both axes and in both layout directions. Two separate inversions: the pane
  order and the drag math have been mirrored since the widget shipped — the
  direction is written into a cell the drag path reads — but the keyboard was
  not, so `ArrowRight` pulled the divider left in a right-to-left window; and
  `increase` is axis-relative, so reading it as a geometric direction made
  `ArrowDown` shrink the pane above a *vertical* divider where it had always
  grown it. Both now resolve through `towards_trailing`, which is named for
  what it answers — "does the leading side get bigger" — rather than for a
  screen axis it does not track once the layout direction is folded in.
  Direction is read at event time, so a locale flip needs no rebuild.
  **Behaviour change.**
- **A `Trailing` or `Bottom` dock side resized in the wrong direction from the
  keyboard.** Its handle sits on the side's inner edge, so growing the side
  moves the handle *towards* the centre; the pointer path already inverted per
  side (`side_main`) and the arrows did not, so they moved the handle the way
  they did not point. **Behaviour change.**
- **A right-to-left `Slider` read every one of its three surfaces
  left-to-right.** A horizontal slider's minimum sits at the *leading* edge,
  which is the right one there, so the fill now grows leftward from a thumb
  that travels the other way, the click-to-jump reads from the other end, and
  the arrows follow. All three key off one answer, read at paint and at event
  time: mirroring any of them alone would leave the `Left` key and a leftward
  drag moving the thumb in opposite directions. A custom `SliderStyle` carries
  the same obligation, which its trait now states. **Behaviour change.**
- **A right-to-left horizontal `ScrollBar` disagreed with its own
  `ScrollArea`.** The area had already mirrored — it anchors content to the
  right and grows `scroll_x` leftward — while the bar placed its thumb at
  `bounds.x + offset`, so at `scroll_x = 0` the content showed its beginning
  and the thumb sat at the far end of the track, and a track click paged the
  wrong way. The thumb, the drag delta, the track-click direction and the
  arrows now key off one predicate. **Behaviour change.**
- `ScrollBar` no longer claims `Action::SetValue`. Its node is `set_hidden()`
  and it never advertised the action, so the arm was unreachable except to
  answer `Handled` to a `SetValue` bubbling up from a descendant and drop it.
- **The right-to-left horizontal `ScrollBar`'s *painted* thumb now mirrors
  too.** The widget's hit-test and drag were mirrored; the style that draws the
  thumb was not, so the thumb was rendered at one end of the track while the
  region that grabs it sat at the other, and it jumped the moment it was
  touched. Both painters in the default `RecipeScrollBarStyle` read
  `PaintContext::layout_direction`, and the trait now states the obligation the
  way `SliderStyle`'s does. **Behaviour change.**
- **A horizontal `ScrollBar`'s `PageUp` scrolled forward and its `PageDown`
  back**, the opposite of the vertical bar beside it. The page keys name a
  direction in the *content*, not on screen, so unlike the arrows they do not
  follow the bar's orientation — reading `increase` geometrically for them was
  what inverted the horizontal case. **Behaviour change.**
- **A vertical `Slider`, `HueStrip` and `AlphaStrip` put their maximum at the
  top.** All three grew their value downward, so `ArrowUp` — which the chord
  table reports as an increase — moved the thumb *down*: the keys and the
  pointer drove the control in opposite directions on screen. Qt's `QSlider`,
  GTK4's `GtkScale`, the Win32 trackbar and `<input type=range>` all put a
  vertical minimum at the bottom, so the geometry moved rather than the chord
  table. The hue strip's rainbow is reversed to match. **Behaviour change.**
- **`Ctrl+Enter` on a `Splitter` divider or a dock resize handle reaches the
  application.** Both matched `Enter` before any modifier test, so the chord
  collapsed a pane or hid a side and reported the key handled — the one row of
  the documented chord table those two did not honour, while every other key on
  them did. **Behaviour change.**
- **An assistive technology's `Slider` write lands on the grid its arrows
  walk.** The advertised `numeric_value_step`, an arrow press and `Increment`
  all move by the effective step — 1 % of the range when none is configured —
  while `SetValue` snapped to the *configured* step, so on a stepless slider a
  write landed between two values every other path could reach, and a screen
  reader then announced a number its own Up arrow could not produce.
  **Behaviour change.**

#### Widgets

- **A `SpinBox` now handles the `Action::SetValue` it advertises.** It carried
  the advertisement — which macOS needs, since AXValue settability is gated on
  it — while its handler could not see an `ActionData` payload at all, so every
  assistive-technology write was dropped: a `NumericValue` from macOS's
  `setAccessibilityValue:` with an `NSNumber` or from AT-SPI's
  `Value.SetCurrentValue`, and a `Value` string from an `NSString` or from
  Teksilo's own automation `set_value` tool, which was a silent no-op against
  every `SpinBox` in the catalog. Both shapes now go through the widget's own
  commit: a number is clamped and published as sent, a string takes the parse
  `Enter` takes, so a custom `value_from_text` and the locale's decimal
  separator are honoured and an unparseable one is *reported* unhandled rather
  than failing quietly. A read-only spin box advertises and services none of
  `Increment` / `Decrement` / `SetValue`.
- **An assistive-technology `SetValue` aimed at a composite's inner text node
  now commits.** A `SpinBox` or a date editor publishes two nodes — the value
  root and the `Role::TextInput` beneath it — and setting the field replaced
  the displayed string and stopped there, so the typed value stayed stale
  until the next blur and `on_value_changed` never fired. The field commits
  through its host now, by the same parse, clamp and revert `Enter` performs.
  A plain `TextInput` is unchanged: its bound signal already *is* the value.
- **A modified letter chord over a focused `ComboBox` no longer changes the
  selection.** Type-ahead matched any key carrying a character and returned
  `Handled`, so an unregistered `Ctrl+C` appended `c` to the prefix, jumped the
  value to the first item starting with it, and blocked every ancestor
  `on_key`. `Ctrl` / `Alt` / `Super` now fall through — `Shift` still types, as
  in `MenuList` — and the same rule covers the arrow, `Home`/`End` and page
  keys, so `Ctrl+Home` no longer picks item 0.
- **`ComboBox` arrows stop at the ends instead of wrapping.** Its page keys
  already clamped, so the widget disagreed with itself inside one handler, and
  wrapping means one keypress too many sends a setting from "Never" to
  "Always". Win32, `QComboBox`, GTK, the ARIA listbox pattern and Teksilo's own
  `ListView` all stop at the ends; menus still wrap, because a menu is a list
  of commands rather than a value. **Behaviour change.**
- **`Alt+ArrowDown` on a `DateEdit` no longer steps the date.** The segment
  stepper reads only `Shift`, so the chord two module docs promised as "opens
  the calendar popover" quietly moved the value back one day. Other `Ctrl` /
  `Super`-modified arrows now fall through instead of stepping.
- **`DateEdit` and `TimeEdit` documented step sizes they have never applied.**
  Both describe *segment*-relative stepping now — one unit of the field under
  the caret, ten with `Shift`, ten on a page key and a hundred on
  `Shift`+page — which is what the code does and what `QDateTimeEdit` does.
  `TimeEdit::step_minutes` is documented as the inert builder it has been since
  segment stepping replaced whole-value stepping.
- `SplitButton`'s `ArrowDown` accepts a bare or `Alt`-modified chord, as its
  comment always claimed; `Ctrl+ArrowDown` and `Cmd+ArrowDown` no longer open
  the menu.
- `SpinBox::read_only` documented that keyboard and button stepping still
  worked. It never did — the keys, the wheel and the buttons are all gated,
  which is what `QAbstractSpinBox::readOnly` does too.
- **A read-only `SpinBox`, `DateEdit`, `TimeEdit`, `DateTimeEdit` or
  `DateRangeEdit` refuses an assistive technology's write on its *inner text
  node* as well as on its root.** Not advertising an action is not the same as
  refusing it: an adapter dispatches what the technology asks for, and AT-SPI
  publishes `EditableText` off the interface set rather than off the action
  list, so a write aimed at the field went straight past the composite's
  read-only gate and rewrote the value. **Behaviour change.**
- **An assistive technology's whole-value write reports the host's verdict.**
  `SetValue` on a text-projected composite answered `Handled` whatever the
  parse said, so `"twelve"` in Orca's value entry and in macOS's
  `setAccessibilityValue:` both read back as success while the field quietly
  reverted to the value it still held. `TextInputField::on_access_set_value`
  now returns whether the host accepted the string. **Behaviour change.**
- **`AltGr`-composed characters reach a `ComboBox`'s and a `MenuList`'s
  type-ahead.** Both refused the union of `Ctrl` / `Alt` / `Super`, and `AltGr`
  arrives as `Ctrl+Alt` on Windows, X11 and Wayland alike — so every character
  behind it (`@` on a German layout, `€` on a French one, `ą` on a Polish one)
  was dead for type-ahead while the unshifted ones kept working. `Ctrl` alone
  and `Alt` alone stay refused. **Behaviour change.**

#### Data views

- `TableView` / `TreeTableView`: dragging a column divider now moves it with
  the pointer. Only the columns after the divider reflow; a `Flex` column
  before it is frozen at its current width in the same `column_widths_signal`
  write. Before, the resized column's delta was shared with every flex column,
  so the divider lagged the pointer (or stood still) while the dividers before
  it slid the other way.
- `TableView` / `TreeTableView`: dropping a dragged column at the very front
  or the very end of a strip with no pinned pane on that side moves it there
  instead of pinning it `Leading` / `Trailing`.
- `TableView` / `TreeTableView`: the header strip's column separators now
  follow a column resize and a horizontal scroll. The strip's bounds change on
  neither, so its cached paint was replayed and the lines stayed where they
  were while the cells moved.
- **A row height a view *declares* is laid out at the height it declares.**
  `ListView::item_height_fn`, `TableView::row_height_fn` and `GridView`'s exact
  `item_height` fed the offset table through the same setter a measurement pass
  uses, whose sub-pixel epsilon exists to stop a re-measure from oscillating the
  scroll anchor. So a declared height within `0.01` px of the table's internal
  placeholder was discarded as jitter and the row kept the placeholder — a list
  of 1.005 px rows laid every row out at 1.0 px, and the arithmetic uniform-height
  mode describing the same geometry then disagreed with it. Declared heights and
  measured ones now take different doors.

#### Core

- **An assistive-technology `Action::Focus` on a composite lands on the node
  that takes the keys.** The dispatcher services `Focus` itself and focused the
  targeted node directly, while `ctx.request_focus` has always walked to the
  first focusable descendant. So an assistive technology — or the automation
  `focus_node` tool — aimed at a `SpinBox`, `DateEdit` or `TextInput` parked
  focus on a composite root that accepts no keystrokes, and, because
  `on_key_preview` fires only on *strict* ancestors of the focused node,
  disarmed the composite's own stepping keys in the process. The walk is taken
  only from a node that **advertises** `Action::Focus`, which is what a
  composite does and a `Panel`, a `GroupBox`, a landmark or a label does not:
  walking in from any non-focusable node moved the keyboard onto the first
  control inside it — one the technology could have named itself and did not —
  and then reported success.
- **An app-installed `.on_access_action(..)` fires on a widget that services
  the payload shape itself.** The dispatcher called
  `on_access_action_request` *instead of* `on_access_action` whenever the
  former was set, so adding an application handler to a `Slider`, `SpinBox`,
  `TextInput`, `CodeEditor` or `TabBar` produced a handler that never ran, with
  nothing at the call site to say so. Every installed slot now fires for one
  dispatched action, and the action counts as handled if any of them says so.

- **A two-finger pinch reaches the zoom it asked for instead of running into
  `max_zoom`.** A spread to twice the starting span now leaves a `SceneView` at
  exactly twice, whatever the sample rate; before, each sample carried the ratio
  to the *start* of the gesture and the handler multiplied every one of them in,
  so a single spread compounded to the product of its intermediate ratios.
- **A trackpad twist turns content by the angle the user twisted.** One degree of
  rotation on the trackpad rotated a `SceneView` by one radian — about 57° —
  because winit reports degrees and the payload is read as radians. The
  conversion now happens where the incoming unit is known.
- `GestureEvent::PinchChanged` and `PinchPhase::Changed` now document `scale` and
  `rotation`: both are deltas against the previous sample, and `rotation` is in
  radians. Fold each sample in (`zoom *= scale`, `rotation += rotation`) rather
  than assigning it.
- **`TouchPinchRecognizer::scale` and `rotation` are renamed
  `cumulative_scale` and `cumulative_rotation`.** They report the totals since
  the gesture started, which is not what a `PinchChanged` carries; the names now
  say which of the two a caller is reading.
- **A `PointerCancel` for a touch contact reported the mouse, with no position**,
  so an application branching on the cancelled pointer's kind behaved wrongly.
- **A container that preserves its children across a rebuild destroyed and rebuilt
  its whole subtree instead**, whenever any builder method wrapped it. That covers
  every `Switcher`, `TabWidget`, `SceneView`, `DockingLayout`, `Repeater` and
  `PopoverWidget` with a handler attached.
- **A wrapped widget's declared shortcuts never reached the registry**, so they
  were missing from the rebinding UI as well as from the keyboard; a wrapped widget
  that opts out of layout memoisation was memoised anyway; a wrapped title bar
  stopped publishing its OS caption regions — a defect its own documentation had
  written up as a rule. Wrapped dialog content stopped lending its title to its
  shell and stopped directing initial focus; the context-menu key opened the
  view's menu rather than the selected row's; a table read its body to assistive
  technology before its header; a scene stopped grafting its items into the
  application's accessibility tree; empty tooltip content still raised a bubble.
- **A coarser density could lower a grip's reach**: an outset and the miss-only
  slop pass were combined in the wrong order.
- **A caret could be left outside a viewport that shrank**; a shrink now records a
  reveal.
- **Two overlay-dismissal defects a mouse could feel**, and a hold dispatched
  under a mouse identity that was never there.


#### Menus

- **Menu item labels line up on one leading inset again under the Fluent and
  macOS presets.** Both stack the shared menu row over their own highlight
  rect, and the row was measured at its content width and centred there — so
  each label sat further in by half its own slack, ragged from row to row.
- **`MenuItemRecipe`'s `icon_column_width`, `padding_horizontal` and
  `separator_height` now reach the parts of the row they describe.** The
  leading icon/check column, the trailing chevron column and `MenuSeparator`
  read the active style through the new
  `MenuItemStyle::metrics() -> MenuItemMetrics` (defaulted, so an existing
  style needs no change) instead of the IntUI module constants. macOS menus
  get their 14 dp check column and 11 dp separator, Fluent its 11 dp trailing
  column and 3 dp `MenuFlyoutSeparatorThemePadding`.
- **A submenu survives the diagonal to reach it.** The safe triangle now
  holds off the submenu overlay's own pointer-leave grace, not just a sibling
  row's hover-switch: the grace used to start the moment the pointer left the
  trigger row and closed the submenu 150 ms later, mid-flight, however slowly
  the user was travelling. It is armed where the diagonal starts — the point
  the pointer leaves the trigger — so a submenu opened by click, Enter or
  ArrowRight is covered too, and it expires after 600 ms so a parked pointer
  releases the menu instead of pinning it.
- **The submenu no longer goes the instant the pointer leaves the trigger
  row.** The safe triangle is a needle at its apex, and the sample that decided
  whether the user was heading for the submenu was the first one off the row —
  a pixel or two out, where the cone is a few degrees wide. For a menu wider
  than its submenu is tall, which is the usual shape, any departure steeper
  than about 30° missed, and one miss was final: the region could only ever be
  armed as the pointer left the row, so nothing could re-arm it. Straying out
  of the cone now merely starts the ordinary close delay, and heading back in
  cancels it; a sibling row stands aside for the whole traversal instead of
  ruling on that one sample. A change of mind still closes the submenu, one
  close delay later rather than at once.
- **Crossing a neighbouring submenu trigger no longer closes the submenu you
  are walking to.** The hover-switch between two submenu triggers now dismisses
  the open submenu when the new one *opens*, not when the pointer first touches
  the row — so passing through costs nothing and settling still swaps on the
  same frame.
- **A submenu trigger stays highlighted while its submenu is on screen.** It
  used to un-highlight as soon as the pointer left the row, which is the whole
  time the submenu is up, leaving the open panel with no visible parent. Its
  `set_expanded` was reporting collapsed over the same window.

#### Widgets

- **A Fluent list or tree row can be clicked again.** The preset's selection pill
  spans the whole row and was hit-tested ahead of the row's contents, so nothing
  inside a Fluent row — a checkbox, a disclosure arrow, a trailing button — took
  a press, by mouse or by finger.
- **A macOS switch and an icon button meet the 24 dp target floor.** Both are
  drawn at the size the preset asks for, Apple's 22 dp track and the 18 dp
  compact icon button among them; the node around the chrome is what grew.
- **Three controls showed no pressed appearance for a pointer at all** — the
  `ToolBox` header, `RadioTile`, and the calendar's month/year cell, the last only
  ever writing `false` into its own state.
- **A declared row height inside the row-metrics dead band was silently refused**,
  so an exact-height list reported a different total height from a uniform one.
- **A leaf row in a tree table no longer carries an invisible pointer target.**
- **The previewer's knob rows were unreachable by any pointer, mouse included**: an
  infinite height cap inside a scroll area measured `inf` tall, its `y` resolved to
  `NaN`, and a rectangle containing `NaN` contains no point.
- **With the debug inspector installed, switching density destroyed the wrapped
  application's tree.**
- **Four dimensions a shipped theme sets now reach the screen.** A table header
  cell pads by the Fluent gutter (12 dp, not 8); a calendar's navigation arrows
  are drawn the size macOS asks for (20 dp, not 24); a search field's suggestion
  rows carry the macOS row gutters (8 x 3 dp, not 10 x 4). Each was written on
  the theme's recipe and discarded, because the widget measured the shipped
  Int UI constant instead of asking the style. `TableStyle`, `CalendarStyle` and
  `SearchFieldStyle` now hand the dimension over, and a custom style keeps the
  ladder it had unless it says otherwise. Int UI at the default density is
  unchanged.
- **A theme may draw a control below the 24 dp target floor without costing the
  app its WCAG 2.2 SC 2.5.8 conformance.** A calendar's navigation arrow keeps
  whatever size the theme asks for and sits inside a box that reaches the floor,
  so macOS's 20 dp stepper is drawn at 20 dp and still answers a 24 dp press —
  from a mouse as much as from a finger. Under Int UI, whose arrow is already
  the floor, nothing moves at any density.

#### Terminal

- **A terminal placed anywhere but the window's top-left corner accepted no
  pointer input at all.**
- **The wheel scrolled the scrollback backwards**, and its report to the child
  program named cell (0, 0) instead of the cell under the pointer — so a
  full-screen program that splits its window scrolled the wrong pane.
- **Dragging a selection handle moved the selection to the neighbouring row.**

#### Previewer

- **The toolbar's Export PNG ignored the live density**, so exporting while the
  previewer was set to Touch produced a Compact image — over the Compact
  filename. The density now reaches both the render and the name.

#### Text

- **`--all-features` builds.** Five `fonts-*` features named a Noto face that is
  not in the repository, and because `include_bytes!` resolves at compile time,
  enabling one was a hard build error — so those five features, both
  `fonts-all` meta-features, and any `--all-features` build of the workspace
  had never compiled on any revision. A face is now embedded only if its file
  is present; enabling a feature without it warns and names the path to drop it
  at, rather than failing the build.
- **Dragging the caret handle moves the on-screen keyboard's candidate window
  with it.** Every editing surface reported the caret's position when a press
  placed it, and none did when a finger *dragged* it, so composing Japanese,
  Chinese or Korean after a handle drag put the candidate list at the caret's
  old position. `RichTextEditor`, `CodeEditor`, `PlainTextEditor`, `TextInput`,
  `PasswordField`, `SearchField`, `SpinBox` and the date/time family; the
  assistive-technology route onto the same handle reports it too.
- **The code editor's gutter returned from inside a clip scope when no text
  backend was installed**, leaving the render frame unbalanced.
- **A `PlainTextEditor` could neither replace nor suppress the context menu its
  documentation offered it**, and the `CodeEditor` family gained the right-click
  menu it never had.
- **`--all-features` builds.** Five `fonts-*` features named a Noto face that is
  not in the repository, and because `include_bytes!` resolves at compile time,
  enabling one was a hard build error — so those five features, both
  `fonts-all` meta-features, and any `--all-features` build of the workspace
  had never compiled on any revision. A face is now embedded only if its file
  is present; enabling a feature without it warns and names the path to drop it
  at, rather than failing the build.

#### Documentation

- **A `ScrollArea` in the `Thin` scroll-bar mode documented a keyboard route it
  does not have.** The bar's arrow / `Home` / `End` / `Page` handlers sit on a node
  that cannot take focus, under every mode — the same limit the
  `ScrollBarPolicy::AlwaysOff` documentation states from the other side.
- **`PointerInfo::primary`'s documentation described the wrong one of three
  similarly-named things.** The field is the W3C *per-kind* flag — a mouse and a
  first finger are both primary at once on a hybrid machine — and the rule it
  carried ("exactly one live pointer, a mouse always wins") belongs to the pointer
  table's own election.
- **`StandardListItem`'s documentation claimed the row reads its recipe's
  projected heights.** It projects the module constants instead; those two recipe
  fields have no reader.
- **The soft-keyboard documentation claimed a touch-placed caret leaves a stale
  IME area standing.** Each editing stack reports the area from its own touch path;
  the unused core method is superseded rather than missing.
- **Two committed catalog images had gone stale**, one of them contradicting a
  safety fix: the message-box preview showed **Yes** as the accented default
  after the widget had switched to **No**, and the colour-picker preview predated
  the widget growing its HSV row.
- **Catalog pages linked to rustdoc module pages that do not exist on docs.rs**,
  because the module is crate-internal; each link now resolves to the nearest
  published module.

#### teksu

- **`teksu!` bodies could not place `gesture_dead_zone` or `keyboard_capture`
  before a child**; both failed with "no method named `child`". A compile-time
  guard now keeps the DSL's builder-method list complete.
- **31 `teksu!` UI fixtures had never run** — a glob naming a directory that does
  not exist, which trybuild reports as "no tests enabled yet" and then passes —
  and one of them was asserting a caret column rustc does not emit.
- **A bare child in any popover produced the generic error** instead of the slot
  hint.

### Known limitations

- **The hardware sign-off is not done.** Everything above that a headless Linux
  host can check is checked by the suite;
  [docs/touch-verification.md](docs/touch-verification.md) is the procedure for
  the rest, and its [sign-off sheet](docs/touch-verification-signoff.md) is empty.
  One question in it can only be answered on a macOS trackpad: whether a positive
  trackpad rotation delta should be negated at the platform seam.
- **A running application does not switch density because a finger arrived.**
  `DensityPolicy::FollowLastPointer` has an ingress and no writer; what a stray
  tap should cost and when hysteresis commits are unanswered.
- **Overscroll is published but not painted.** `ScrollableAxes::overscroll`
  carries the value; nothing renders a stretch or a glow.
- **A plain `ScrollArea` has no keyboard scroll route.** Assistive technology can
  scroll it and the data views bring their own key handling, but a keyboard user
  facing a scroll region whose content holds no focus has none — a pre-existing
  WCAG 2.1.1 gap, now written down.
- **A finger cannot drag a window on Wayland.** No protocol for it exists.
- **The target-size gate is green for three crates' named fixtures**, not for the
  framework: four crates that own targets have no fixture list, and the lists
  install one preset.
- **Reordering a data-view row with a finger is unreliable on short rows.** A
  deferred drag is revoked if the first sample after the hold leaves the pressed
  row, and the touch drag slop is wider than a default tree row.
- **`cargo check --workspace --all-features` does not build on any revision**,
  including before this release: five of the seven optional fallback font faces
  are named by the build and were never committed.
- The full ledger, each entry with its measurement, is
  [Touch & pen](docs/touch-and-pen.md) §10.

#### Accessibility

- Eleven labels announced their control's name a second time — in `Banner`,
  the docking activity bar (twice), `MessageBox`, `SearchField`, `Stepper`, the
  calendar's zoom cell, a tab header, `Toast`, `Toggle` and `TooltipWidget`.
  They are hidden now; the control keeps the name.
- The privacy-settings rows painted their label twice on screen.
- A markup label announced its raw source — brackets, parentheses and URL and
  all — instead of its rendered text, and its links were labelled with a
  mis-sliced fragment of that source and re-identified whenever the label
  rewrapped.
- A label bound to a signal changed on screen without telling assistive
  technology, and a widget that moved, resized, or was re-themed or re-scaled
  went on reporting the position and metrics it had before.
- A `labelled_by` relation pointing at a node absent from the tree — a dormant
  tab panel, a pruned stack — crashed the consumer as it built the name.
- The rich-text and code editors emitted no text-change events at all: their
  runs sat under `Role::Paragraph` nodes, and a run's update routes to its
  filtered parent, which supports no text ranges.

#### Layout

- `FormLayout` survives a rebuild. Its rows are handed in once and cannot be
  reconstructed, so it now re-attaches them rather than re-deriving them —
  before, any rebuild (a locale switch, a signal on an ancestor) left an empty
  form measuring zero by zero, and took whatever the user had typed with it.

## [0.9.4] - 2026-09-05

One fix, to the fallback the automation bridge takes when `$TMPDIR` is too deep
to hold a socket: it could not bind at all on macOS, and on Linux it reached
for a permission change on `/tmp` that was never its to make.

### Fixed

#### Automation

- **The short `/tmp` fallback tried to make `/tmp` itself owner-only.** The
  bridge drops to `/tmp/tka-<pid>` when `$TMPDIR` would overflow `sun_path`,
  and then tightened that path's *parent*. On macOS it could therefore never
  bind: `/tmp` is a symlink, which the check rejects by design, the target
  not being what it would be protecting. On Linux the same call chmods a
  `1777` `/tmp` to `0700` — `EPERM` for a normal user, and a silent success,
  for every other process on the machine, for one running as root. Only the
  bridge's own descriptor directory is tightened now; the per-process socket
  directory underneath is still created `0700`.

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
- **A read deadline on the Unix socket expired as `WouldBlock`**, not as the
  `TimedOut` the transport trait documents and the Windows named pipe already
  returns.

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

[Unreleased]: https://github.com/FernTech-EU/teksilo/compare/v0.9.5...HEAD
[0.9.5]: https://github.com/FernTech-EU/teksilo/compare/v0.9.4...v0.9.5
[0.9.4]: https://github.com/FernTech-EU/teksilo/compare/v0.9.3...v0.9.4
[0.9.3]: https://github.com/FernTech-EU/teksilo/compare/v0.9.2...v0.9.3
[0.9.2]: https://github.com/FernTech-EU/teksilo/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/FernTech-EU/teksilo/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/FernTech-EU/teksilo/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/FernTech-EU/teksilo/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/FernTech-EU/teksilo/compare/v0.6.2...v0.7.0
