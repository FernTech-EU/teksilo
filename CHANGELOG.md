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

## [0.9.2] - 2026-09-03

Accessibility. Every entry below changes what a screen reader says, and four
of them fix something that reached no assistive client at all. Two were found
by driving a real screen reader after the widget tests had already passed, and
both fixes are shaped by what the AccessKit adapters actually do rather than by
what the documentation says they do.

**Added**

- [`TextInput::field_id`](#added-teksilo-widgets-a-host-can-name-the-node-a-textinput-actually-focuses),
  so a form can send focus to the field a validator refused, and a modal can
  pick one field out of several. `TextInput` also answers `initial_focus_hint`
  now.
- [`WindowConfig::app_id`](#added-teksilo-core-a-window-can-say-which-application-it-is),
  the identity a desktop matches a window against. Without it a Wayland
  toplevel sends no `app_id` at all and the shell shows an unnamed window with
  the fallback icon.

**Fixed**

- [A labelled `TextInput` was nameless to every screen reader](#fixed-teksilo-widgets-a-labelled-textinput-was-nameless-to-every-screen-reader).
  **Behaviour change** in what a screen reader says: fields labelled through
  `.label(..)` now have a name where they had none.
- [Nothing focusable advertised that it could be focused](#fixed-teksilo-core-nothing-focusable-advertised-that-it-could-be-focused),
  so assistive technology could not put focus in a list, tree, table, grid or
  open menu. **Behaviour change:** every focusable node now carries
  `Action::Focus`.
- [No data view told a screen reader which row was current](#fixed-teksilo-widgets-no-data-view-told-a-screen-reader-which-row-was-current).
  Arrowing through a `ListView` was silent to NVDA. **Behaviour change:** all
  five views nominate the current row as their active descendant and reveal it
  when they take focus.
- [A selection move replaced every row node in the list](#fixed-teksilo-widgets-a-selection-move-replaced-every-row-node-in-the-list),
  resetting the scroll offset and the keyboard anchor, and handing the
  accessibility tree a fresh set of node ids on every keystroke.
- [A multi-select view reported that it could hold only one row](#fixed-teksilo-widgets-a-multi-select-view-reported-that-it-could-hold-only-one-row).

### Added (`teksilo-widgets`): a host can name the node a `TextInput` actually focuses

`TextInput` is a composite. The id `ctx.add` returns is the outer node's, and
that node is a `Role::GenericContainer`: not focusable, and dropped from the
filtered accessibility tree. So an application that had to *name* the focus
target had nothing to name it with.

Two ordinary cases need one: a form sending focus back to the field a validator
refused, and a modal with several fields choosing one for `initial_focus_hint`.
An application meeting this had to give up on `TextInput` and assemble
`TextInputField` plus the theme's own chrome by hand, only to arrive at an id
the framework already had.

`field_id()` returns a shared slot filled in by `build`, in the same shape as
`caret_setter` and `handle`: take it before `ctx.add`, read it after.
`TextInput` also answers `initial_focus_hint` with that field now, because the
modal presentation pipeline asks the content tree for a hint before falling
back to the first focusable descendant, and a composite that answered nothing
was skipped over. Deferred content whose one control was a text field opened
with focus nowhere in particular.

A host that only means "focus this input, whichever node that is" still wants
`request_focus_into` on the outer id and no handle at all.

### Added (`teksilo-core`): a window can say which application it is

A Teksilo window on Wayland sent no `xdg_toplevel.set_app_id`, ever. winit sends
one only when `WindowAttributesExtWayland::with_name` has been called, and
nothing here called it, so a compositor had nothing to tie the window to its
desktop entry with.

What that costs is the application's whole desktop identity. The shell shows the
window as a separate, unnamed entry in the dash and in Alt+Tab, carrying the
generic fallback icon however many icons were installed. `StartupWMClass` in the
desktop entry does not rescue it: there is no `WM_CLASS` on Wayland to put
there, and GNOME reads that key for X11 windows only. Portals attribute
notifications by the same id, so those were unattributed too.

It goes unnoticed easily. On X11 winit falls back to the executable's own name,
which is usually right, and KWin falls back the same way, so a KDE session looks
correct while GNOME does not.

`WindowConfig::app_id` defaults to `None`, which leaves winit's behaviour
exactly as it was. Set it to the basename of the installed desktop entry. One
call covers both display servers, since winit's Wayland and X11 extension traits
write the same field. Windows and macOS identify a window no such way and ignore
it.

### Fixed (`teksilo-widgets`): a labelled `TextInput` was nameless to every screen reader

`TextInput::label` wrote the accessible name onto the composite's own node,
which carries `Role::GenericContainer`. `accesskit_consumer`'s `common_filter`
(`accesskit_consumer-0.39.0/src/filters.rs:31-33`) returns `ExcludeNode` for
that role **unconditionally**, before it looks at anything else on the node, and
the only escape above it, `is_focused()`, never applied here, because the outer
node is not focusable; the inner `TextInputField` is. So the name was discarded
on every platform, always. A field labelled through the documented API announced
as an unnamed edit box.

The name now goes on the inner field, which carries `Role::TextInput`, holds
focus, and survives the filter, which is what `PasswordField` was already doing.
Routing it through `access_label` also makes the name locale-reactive, which the
previous `resolve_now()` snapshot was not: a `tr!(..)` label now follows a locale
change without a rebuild.

`TimeEdit`, `HexColorInput`, `FilePickerField`, `SearchField` and `FontPicker`
all forward into `TextInput::label`, so all five are fixed by the same change.
An app working around this by naming the field itself can drop the workaround;
the explicit `access_label` still wins, so nothing breaks if it does not.

A sweep for the same shape found two more, both in the previewer UI:
`PreviewCanvas` and the inspector body each named a `GenericContainer`. Both now
use `Role::Group`, the rule `FormLayout` already followed when it upgrades to
`Role::Form` for a labelled form. **A name belongs on a role that survives the
filter**; `GenericContainer` is for nodes that carry no semantics at all.

### Fixed (`teksilo-core`): nothing focusable advertised that it could be focused

`WidgetTree` services `Action::Focus` itself, the `AccessAction` arm calling
`focus_with_origin_ops` rather than handing the action to the widget, so an
assistive technology that *tried* the action got focus. Nothing ever told it the
action was there, and an AT offers what a node advertises. Focus was reachable
and undiscoverable at the same time.

Advertising it was left to each widget, which made it a rule to be remembered
about forty times, and it was not. Every stock button remembered. `ListView`,
`TreeView`, `TableView`, `TreeTableView`, `GridView`, `MenuList` and
`OverlayTrigger` did not, so a screen reader could not programmatically put
focus in a list, a tree, a table, a grid, or an open menu. Each of those is a
focusable container whose rows, cells and items are deliberately *not* focusable,
which makes the container the only node that can hold focus and therefore the
only one that could have advertised it.

The advertisement is now derived from the arena's own focusable flag, in
`announce_focusable`, the sibling of `announce_context_menu`, in the same place,
for the same reason. Gated on the arena flag rather than the widget's opinion,
because that flag is what `focus_with_origin_ops` will actually honour: a node
that is not focusable would otherwise advertise an action that then declines to
land. It runs before the overrides, so `access_remove_action(Action::Focus)`
still takes it away. Both parallel builder paths derive it, so a node reads the
same to a test as to a screen reader.

Fixing it per widget was the wrong shape: it would have closed the seven known
holes and left the rule waiting to be forgotten by the next focusable widget
written. Derived from the flag, the advertisement is true by construction, and
the per-widget line is gone from all five data views.

### Fixed (`teksilo-widgets`): no data view told a screen reader which row was current

Arrowing through a `ListView` was completely silent to NVDA. Confirmed against a
real one: it spoke the list on arrival, and an arrow press produced no speech
line at all, while hovering the mouse over the same row read the whole sentence.
That last part is what identifies the fault. Hit testing reads the tree
directly, so names, roles and structure were all correct; what was missing was
an event.

A view keeps keyboard focus on itself and marks the current row `selected`. On
AT-SPI that is the whole story, since Orca announces a selection change. UIA has
no selection-driven announcement to match it, and no active-descendant property
either. What it has is a focused element, and for a list box that element is the
item.

AccessKit bridges the two in the consumer rather than in each adapter:
`accesskit_consumer` resolves the focused node as
`focused.active_descendant().unwrap_or(focused)`
(`accesskit_consumer-0.39.0/src/tree.rs:541`) before telling an adapter the
focus moved, and `accesskit_windows::focus_moved` raises
`UIA_AutomationFocusChangedEventId` on whatever comes out
(`accesskit_windows-0.35.0/src/adapter.rs:341-345`). So a container nominating a
row turns every arrow press into the focus change a screen reader announces,
without keyboard focus moving anywhere, and `is_focused` moves from the
container to the row exactly as the ARIA listbox pattern says it should.

The nomination is gated on the view actually holding focus. The combobox pattern
keeps focus on a text field pointing into the same list, and two publishers of
one row would otherwise appear in the tree.

Nominating a row cannot help while that row has no widget to nominate, which is
the other half. Only the rows near the viewport are realized, so a selection
made before the view is looked at, a restored session or a landing on the
current item, usually sits outside that window. Nothing then carries `selected`,
there is no node to point at, and the first arrow press steps past the row
because the cursor was somewhere nobody was shown. Every view now reveals its
current row when it takes focus, by ensure-visible rather than scroll-to, so a
row already on screen does not lurch under somebody who can see it.

`ListView` reveals on a selection change as well, and the second trigger is not
redundant. A list that lands on whatever is happening now makes that selection
from *inside* its own focus handler, so a focus-only reveal runs first, finds
nothing selected, and does nothing. The selection trigger is gated on focus, so
a selection driven from elsewhere never scrolls a view the user is not in. This
half was found by driving a real application: the widget test passed while the
application stayed silent, because the test selected before focusing and the
application selects after.

`TreeView` nominated no active descendant at all and now does. `TableView`,
`TreeTableView` and `GridView` nominated one but revealed nothing; all three now
reveal, by row and by tile. `TableView`'s reveal is vertical only, and the doc
says why: its body pane realizes every display column of a realized row, so the
cell is in `cell_map` whatever the horizontal offset is, and the row is the only
axis that can hide it from the AT tree.

`TreeView` still publishes no `size_of_set`, and the comment there argues it
now instead of being silent about it. A flattened tree cannot express "the 2nd
of 5 siblings" from one container value, because AccessKit resolves an item's
set size by walking up from it, so the only number the container could carry is
one shared by every row at every depth. Doing it properly needs a `Role::Group`
per expanded branch, which changes the tree shape for every tree widget and is
its own decision.

### Fixed (`teksilo-widgets`): a selection move replaced every row node in the list

Every arrow press replaced every realized row. `ListBodyPane` watched the
selection and rebuilt itself, and a rebuild destroys the row widgets and their
arena nodes, so each keystroke handed the accessibility tree a fresh set of node
ids for content that had not changed.

That is expensive in three ways, and the third is the one that matters. It reset
the scroll offset. It reset the keyboard anchor. And it meant the
`active_descendant` the container nominates named a node the screen reader had
never been told about, one keystroke after the node it had been told about
ceased to exist. The nomination above made that visible; it was there before,
unnoticed, because nothing was reading the ids.

Only two rows change when the selection moves: the one that lost it and the one
that gained it. So `ListItemWrapper` is now the rebuild boundary. It builds the
delegate's row widget itself instead of being handed one, and watches the
selection for its own row alone, rebuilding only when its own state flips. Every
other row keeps its widget, its arena node and its node id.

The pane keeps its per-row handlers, which were already applied to the wrapper's
id rather than to the row widget inside it. That is what makes this safe: they
survive a rebuild of the wrapper's children, and they are never applied twice.
`HandlerSet::merge` chains handlers rather than replacing them, so a row whose
handlers were reapplied would act on one press twice.

### Fixed (`teksilo-widgets`): a multi-select view reported that it could hold only one row

`GridView` published `multiselectable`; `ListView`, `TreeView` and
`TreeTableView` never did. Left unset it reads false, so a view configured for
multi-select told every screen reader that one row was the most it would ever
hold. The property is real on both platforms that have one: UIA's
`SelectionCanSelectMultiple` and AT-SPI's multiselectable state.

It is gated on the selection mode, exactly as `GridView` gates it, and the gate
is load-bearing rather than tidy. `accesskit_windows` picks the event it raises
on a selection change from this very property
(`accesskit_windows-0.35.0/src/adapter.rs:189-199`):
`ElementAddedToSelection` when it is true, `ElementSelected` when it is false. A
single-select view publishing it unconditionally would trade the right event for
the wrong one.

`TableView` is deliberately left out. It publishes `Role::Table`, which is not
among the roles `accesskit_consumer::is_container_with_selectable_children`
recognises (`accesskit_consumer-0.39.0/src/node.rs:938-955`), so it exposes no
selection for the property to describe and the line would be dead. Whether it
should publish `Role::Grid` instead is a real question, of the same shape as
List versus ListBox, and it changes what every existing application announces,
so it is not settled here.

## [0.9.1] - 2026-09-02

**Added**

- [crates.io metadata on every crate](#added-packaging-cratesio-metadata-on-every-crate): `homepage`, `documentation`,
  `keywords` and `categories`, so each crate links to its own docs.rs page.
- [A context menu can be opened from the keyboard](#added--a-context-menu-can-be-opened-from-the-keyboard): the Menu key,
  Shift+F10, and Ctrl+Shift+M on macOS, on every widget that already has a
  `.context_menu(..)`. **Breaking:** new `Key::ContextMenu` variant, and a
  settings file containing it does not load on an older build.
- [`ctx.announce(...)`](#added--teksilo-core-ctxannounce-a-live-region-that-actually-speaks), an announcement that reaches a screen
  reader on all three platforms, including repeats of the same message.
- [`WeakEditorHandle`](#added-teksilo-widgets-weakeditorhandle-a-handle-that-does-not-keep-its-editor-alive), for a rich-text handler that must not keep
  its own editor alive.

**Changed**

- [`text-document` 1.12 and `text-typeset` 1.10](#changed-text-document-112-and-text-typeset-110).
- [A failed `assert_node` is a failure on every automation transport](#changed--a-failed-assert_node-is-a-failure-on-every-transport).
  **Breaking:** callers that read `.passed` off an `Ok` reply.
- [`MessageBoxButtons::YesNo` and `YesNoCancel` default to No](#changed--teksilo-widgets-a-yesno-message-box-no-longer-defaults-to-yes).
  **Behaviour change:** Enter on a confirmation dialog now declines.
- [A font face is registered without copying its bytes](#changed-teksilo-text-a-font-face-is-registered-without-copying-its-bytes).
  **Breaking:** `FontFaceSpec::data` is now `SharedFontData`, which
  `Arc<Vec<u8>>` still coerces to.

**Fixed**

- [53 places where the documentation contradicted the code](#fixed-docs-53-places-where-the-documentation-contradicted-the-code),
  including three `teksilo = "0.7"` version references in the app guide and a
  minimal-build recipe that did not compile.
- [No collection ever announced "of N"](#fixed--no-teksilo-collection-ever-announced-of-n-and-listview-announced-nothing-at-all), and `ListView` published
  no position at all. **Behaviour change** in what a screen reader says.
- [Every published position, row, column and level was one too high](#fixed--every-position-index-and-level-teksilo-published-was-one-too-high).
  **Behaviour change:** all four move down by one, to the values they should
  always have had.
- [A keystroke no longer recolours the whole rich-text document](#fixed-teksilo-widgets-a-keystroke-no-longer-recolours-the-whole-document).
- [A closed window releases its event subscriptions](#fixed-teksilo-core-a-closed-window-releases-its-event-subscriptions) instead of
  holding their captures for the life of the process.
- [A long category label no longer starves a chart's plot to nothing](#fixed-teksilo-charts-a-long-category-label-no-longer-starves-the-plot).
- [Observing a derived signal no longer panics](#fixed-teksilo-core-observing-a-derived-signal-no-longer-panics), which aborted a
  macOS application before its first window was drawn.
- [The X11 drag-and-drop teardown is confirmed by the server](#fixed-teksilo-platform-the-x11-drag-and-drop-teardown-is-confirmed-by-the-server), so
  `XdndProxy` cannot outlive the window it points at.
- [Publish order counts dev-dependencies and the umbrella crate](#fixed-release-publish-order-accounts-for-dev-dependencies-and-the-umbrella-crate),
  the ordering bug that failed three crates mid-release for 0.9.0.

### Changed: `text-document` 1.12 and `text-typeset` 1.10

The two sibling text crates move to their latest minor versions. Both are
version-pinned path dependencies, so a published build resolves them from
crates.io at those minors.

### Added (packaging): crates.io metadata on every crate

The workspace root gained a `homepage` (`https://teksilo.rs`), and its
`repository` URL is now the canonical
`https://github.com/FernTech-EU/teksilo.git`. Every crate declares `homepage`,
`documentation`, `keywords` and `categories`, so each one links to its own
docs.rs page and can be found by search rather than by name alone.

### Fixed (docs): 53 places where the documentation contradicted the code

A sweep read every module doc, every public rustdoc comment, every Markdown page
under `docs/`, the app guide, `CONTRIBUTING` and the examples, and checked each
factual claim against the code beside it. Each finding was then handed to an
independent reader told to refute it. 53 survived. These are not gaps or
vagueness: each is a statement a reader would act on and be wrong.

The ones that would have cost the most:

- The app guide said `teksilo = "0.7"` in three places, against a 0.9 workspace.
- The guide said builder-call order is irrelevant. It is not.
  `install_toast_default()` resolves `AppPaths` inside the call and panics
  there, so `.application(...)` has to come first. The guide's own snippet did
  not.
- The guide's minimal-build recipe dropped the `i18n` feature, which removes
  `tr!` and `lit!`. `LocalizedString` has no `From<&str>`, so every labelled
  widget in the recipe stops compiling.
- `Calendar`'s doc told readers to call `set_label`. The method is `set_name`.
- `Expand::flex`'s own doc wrote `flex(0)`, which does not compile against an
  `f32` parameter. So did the guide, twice.
- `BuildContext`'s "# Panics" block sat on a private helper that cannot panic,
  while `subscribe_event`, which can, carried none.
- Three examples and `DropZone` said X11 has no drag-and-drop backend. XDND is
  in `teksilo-platform/src/external_dnd.rs`, and the Browse button they
  described as a fallback is the keyboard route.
- `sync_accessibility` claimed it "only rebuilds when layout has changed". A
  plain relayout has not invalidated that cache for some time; the real trigger
  list is nine other things.
- The teksu language spec used `ButtonVariant::Default`, `Regular` and `Flat` in
  19 places. The variants are `Plain` and `Ghost`, and the builder is
  `.variant(...)`, not `.style(...)`.

Roughly a third of the findings follow from the five accessibility fixes below,
which is why this landed last: documentation describing `size_of_set` on each
item, or a tree row announcing "2 of 5", now describes something the code
deliberately no longer does.

Four dead fields went with them, under the same rule that produced the findings.
`TileA11y::total`, `TreeRowA11y::size_of_set`, `SuggestionRow::total` and
`TreeItemWrapper::total_siblings` were each constructed on every build and read
by nothing, left behind when their property moved to the container.

The repository also had two `typos` configurations. `.github/typos.toml` was the
one CI named, and a thin `_typos.toml` at the root allowlisted two words.
`typos` searches the working directory and its parents and nowhere else, so a
config under `.github/` is reachable only by passing `--config` explicitly, and
a developer running `typos` locally got the thin one plus a page of failures CI
never reported. There is now one config, at the root, used by both.

The widget catalog under `docs/widgets/` is regenerated from the corrected
module docs.

Two smaller documentation fixes landed alongside. `cargo doc` is clean again:
the `pub` method `purge_subscriptions_for_window` linked the `pub(crate)`
`WidgetTree::destroy_subtree`, which trips rustdoc's `private_intra_doc_links`
lint, and it now points at `BuildContext::destroy_subtree`. And
`docs/styling-system.md` no longer describes Material 3 as unshipped, two
sentences left over from before the Material 3, Fluent and macOS presets
existed.

### Changed — a failed `assert_node` is a failure, on every transport

`AutomationOp::AssertNode` returned a false assertion as
`Ok(AssertionResult { passed: false })`. A caller that forgot to unwrap the
payload and check `.passed` got a green result for a failed assertion, which is
the worst possible default for a testing tool.

The MCP server carried a bolt-on that re-read its own JSON payload to set
`is_error`; the socket bridge and every direct `execute` caller had nothing at
all. The decision now happens in the toolkit, so every transport inherits it:
`AutomationReply::Err { code: "ASSERTION_FAILED", message }`, which rmcp already
maps to `CallToolResult::error`, so `isError` falls out with no special case and
the bolt-on is deleted rather than patched.

`Ok(CallToolResult::error(..))` rather than `Err(ErrorData)` is deliberate and
is what rmcp's own documentation prescribes (`rmcp-2.2.0/src/model.rs:3006-3026`):
the caller sees the message, where an `ErrorData` is rendered opaquely.

**"Assertion false" and "node does not exist" stop looking identical.** A
property assertion against a node that is not in the tree is `NOT_FOUND` — a bad
node reference, not a property mismatch — while a real node whose property did
not match is `ASSERTION_FAILED`, with the actual and expected values in the
message. `kind: "exists"` against a missing node is the one case that stays
`ASSERTION_FAILED`: asking whether something exists and being told it does not
is an answer, not a lookup error.

Callers that matched on `Ok` and read `.passed` need updating. A passing
assertion is unchanged.

### Fixed — no Teksilo collection ever announced "of N", and ListView announced nothing at all

AccessKit puts `size_of_set` on the **container**, unlike ARIA's per-item
`aria-setsize`: `size_of_set_from_container`
(`accesskit_consumer-0.39.0/src/node.rs:629-641`) resolves an item's set size by
walking *up* from its parent. Every one of Teksilo's fifteen writes was on an
item, and no container anywhere set it. So the property was inert, on every
platform, for every widget — a tab said "tab 3", never "tab 3 of 5"; a dropdown
said "Apple, selected", never "1 of 12"; a wizard said "Account details", never
"step 2 of 4".

The count now lives on the container in every family: `ListView`'s
`Role::ListBox`, the tab bar's `Role::TabList`, the combo panel's `Role::ListBox`
(fed by whichever build path ran), `SearchField`'s suggestion list, the code
editor's completion popup, `GridView`'s `Role::Grid` beside its row and column
counts, `ColumnFlow`'s `Role::List`, `SegmentedControl`'s and
`RadioTileGroup`'s `Role::RadioGroup`, the stepper's indicator strip, and the
docking rail's `Role::TabList`, `Role::Toolbar` and `Role::Menu`. The item-side
writes are gone; each carries a comment saying where its half went.

**`ListView` set neither half.** No row published a position and no container a
total, so a screen-reader user arrowing through a 200-row list heard each row's
label and nothing at all about where they were. Rows now publish their position
in the **model**, not in the realized virtualization window — scroll to the
150th row and it says 151, not 1 — and the container publishes the logical
length.

**The two tree families lose a write that never worked, and gain nothing.** A
flattened tree publishes every visible row as a sibling of one container, so the
only set size it could express is one shared by every row at every depth, which
is not what "the 2nd of 5 siblings" means. Doing it properly needs a real
`Role::Group` node per expanded branch, which changes the AT tree shape for
every tree widget and every consumer of it. Until then, `TreeView` and
`TreeTableView` no longer write a number no adapter reads: the level, the
expanded state and the sibling position still carry the hierarchy, and a
missing feature no longer looks like a working one.

The tests moved with the code. They used to read `size_of_set` straight off the
item's node, which is precisely how fifteen inert writes stayed green for as
long as they did. A new `a11y_set_semantics` helper asks the question the way a
platform adapter asks it — position off the item, size resolved by the
consumer's own upward walk — and every rewritten test goes through it.

### Added — a context menu can be opened from the keyboard

There was no keyboard route to a context menu at all. `Key` had no
`ContextMenu` variant, nothing listened for Shift+F10, and the assistive-
technology route is dead too: `Action::ShowContextMenu` appears in **zero** of
`accesskit_windows-0.35.0`, `accesskit_macos-0.27.0` and
`accesskit_atspi_common-0.20.0`. A menu reachable only by right-click is a menu
a keyboard user does not have.

Three chords are now reserved at the dispatcher, so every widget with a
`.context_menu(..)` gets one without opting in:

- **The dedicated Menu key** (`Key::ContextMenu`, new): `VK_APPS` on Windows,
  `keysyms::Menu` on X11 and Wayland. macOS never produces it —
  `winit-0.30.13`'s AppKit backend references the variant zero times.
- **Shift+F10**, the convention Windows, GTK and Qt all honour, and the only
  route on a PC keyboard with no Menu key.
- **Ctrl+Shift+M on macOS**, where neither of the above exists: Mac keyboards
  have no Menu key, and F10 is a media key under the default function-key
  setting, so Shift+F10 may never arrive as F10 at all.

Modifiers are matched exactly, so Ctrl+Shift+F10 still belongs to the
application, and the chords sit *below* shortcut resolution, so an application
that deliberately binds Shift+F10 keeps it. When nothing on the ancestor chain
owns a factory the key falls through to normal dispatch untouched.

**Data views nominate the row, not themselves.** A `ListView`, `TreeView`,
`TableView`, `TreeTableView` or `GridView` is focusable as a whole and its rows
deliberately are not — the container owns focus and `set_selected` is what tells
assistive technology which row is current. So "open the focused widget's menu"
would have opened the *list's* menu, in exactly the widget family where a
per-row menu matters most. A new `Widget::context_menu_key_target` (default
`None`) lets a widget nominate a descendant instead; all five views implement it
against their keyboard cursor, falling back to the first selected row. The menu
is anchored at the target's own bounds, because a keyboard user has no pointer
position and the stale one would put the menu somewhere unrelated.

**Semver.** `Key` is public and not `#[non_exhaustive]`, so a new variant breaks
an exhaustive match. More importantly it derives `Serialize`/`Deserialize` and
is persisted in shortcut settings: a settings file written by this build that
contains `ContextMenu` **fails to load on an older build**. That is a real
hazard for anyone who downgrades, and there is no forward-compatible encoding
available without changing the settings format.

### Fixed — every position, index and level Teksilo published was one too high

`AccessNodeBuilder` forwarded ARIA-shaped ordinals straight into AccessKit
properties that are zero-based. AccessKit says so itself, in four separate
"**Difference with ARIA**" paragraphs (`accesskit-0.25.0/src/lib.rs`):

| property | ARIA | AccessKit |
|---|---|---|
| `position_in_set` / `aria-posinset` | 1-based | **0-based** |
| `row_index` / `aria-rowindex` | 1-based | **0-based** |
| `column_index` / `aria-colindex` | 1-based | **0-based** |
| `level` / `aria-level` | 1-based | **0-based** |

Both consuming adapters add the 1 back before speaking
(`accesskit_windows-0.35.0/src/node.rs:682-687` and `:698-701`,
`accesskit_atspi_common-0.20.0/src/node.rs:394`). So on Windows and Linux the
first tab of five announced as "tab 2", the first row of a table as row 2, an
`<h1>` as "heading level 2", and a root tree item as "level 2" — with no way for
any Teksilo application to say "level 1" at all. `accesskit_macos-0.27.0` reads
none of the four, which is part of why it went unseen: testing on a Mac shows
nothing wrong.

Fourteen call sites across `ListView`, `TreeView`, `TableView`,
`TreeTableView`, `GridView`, `TabWidget`, `SegmentedControl`, `ComboBox`,
`RadioTileGroup`, `Stepper`, `ColumnFlow`, `SearchField`, the docking activity
bar and the code editor were affected — every collection widget the framework
has.

**The public API is unchanged and stays 1-based.** `set_position_in_set(3)`
still means "the third item", which is what ARIA means, what every call site
already passed, and what a person would say. The conversion now happens once, at
the `AccessNodeBuilder` boundary, so no widget carries it. A new
`AccessNodeBuilder::set_level` joins the other three, because heading and tree
levels were reaching the raw node directly.

**This changes what a screen reader says for every existing Teksilo
application.** Positions, table coordinates, heading levels and tree depths all
move down by one, to the values they should always have had. No source change is
needed to pick that up. If an application hand-compensated for the old behaviour
by passing a 0-based value, that compensation now reads one too low and should be
removed.

Two things went with it. `set_child_position_in_set` now writes the set size on
the widget's **own** node rather than on the child: AccessKit resolves a set size
by walking *up* from an item (`accesskit_consumer-0.39.0/src/node.rs:629-641`),
so a size written on the item is read by no adapter on any platform. And the
heading clamp, which pinned levels to 1..=6 before writing them through, made
AccessKit level 0 — an `<h1>` — unreachable; it now clamps then converts.

A new integration test, `teksilo-widgets/tests/aria_ordinal_conventions.rs`,
fails the build if any widget reaches past the typed setter with
`inner_mut().set_position_in_set(..)` and friends. That bypass is what let the
off-by-one spread to fourteen sites while each one looked locally correct.

### Added — `teksilo-core`: `ctx.announce(...)`, a live region that actually speaks

An application often needs to tell the user something that is not the name of any
widget: "Event added", "Undone: delete event", "Row moved to position 3 of 12".
There was no way to do that, and every attempt to build one out of
`access_live(Live::Polite)` was silent on at least one platform.

```rust,ignore
ctx.announce(tr!(event_added(title = title.clone())));
ctx.announce_with(tr!(save_failed()), Politeness::Assertive);
```

Available on `EventContext`, `BuildContext` and `WidgetTree`. Takes
`impl Into<String>`, so `tr!(...)` works directly; deliberately not a
`LocalizedString`, since an announcement is an event rather than a label and
re-resolving it on a later language switch would re-speak it.

Why it has to be in the framework, read out of the three adapters:

| | node enters the filtered tree | its label changes in place |
|---|---|---|
| Windows `accesskit_windows-0.35.0` | announces | announces |
| macOS `accesskit_macos-0.27.0` | announces | announces |
| Linux `accesskit_atspi_common-0.20.0` | announces | **never** |

The AT-SPI adapter emits `ObjectEvent::Announcement` from `add_node` and from
nowhere else; its `node_updated` says nothing about `live` at all. So on Linux,
editing a live region's text announces nothing. Meanwhile Windows and macOS both
require the label to have *changed*, so repeating a message is silent there.

The one mechanism that satisfies all three, for a new message and for a repeat,
is to retract the node and put it back. The framework owns two reserved AccessKit
nodes — one polite, one assertive — and hides and re-exposes them, which
`common_filter` turns into a real exit from and re-entry into the filtered tree.
Each message costs two accessibility syncs, both scheduled automatically.

Messages queue rather than replace one another: two things happening in quick
succession are two things the user needs to hear.

**Do not add `announce()` beside an existing `show_toast()` on the same path.**
`Toast` is already a correct live region — a node that appears — so an
application that does both says everything twice, and no automatic detection is
possible from either side.

Also added: `WidgetTree::accessibility_tree_snapshot`, which builds a
`TreeUpdate` for inspection without caching it, bumping the AT version, recording
announcements or advancing the live regions. The automation screenshot path now
uses it, because going through `sync_accessibility` there would consume an
announcement the user was meant to hear.

### Changed — `teksilo-widgets`: a Yes/No message box no longer defaults to Yes

`MessageBoxButtons::YesNo` and `YesNoCancel` now take **No** as their preset
default button. Enter on a confirmation dialog therefore declines instead of
accepting, and `No` receives initial focus on open.

This is a behaviour change for any application that used either preset without
calling `default_button`. It is deliberate. A Yes/No box asks a question the
user did not initiate, and it is overwhelmingly asked before something
irreversible; the previous default meant Enter deleted, discarded or overwrote
on a dialog the user had not finished reading. Every one of Teksilo's own
examples that uses the preset — `close_confirmation`, `dialogs_and_popovers`,
`tab_widget` — already overrode it to `No` by hand, which is the clearest
evidence available that the default was wrong.

It matters most for screen-reader users, because no platform tells them which
button is the default: `Node::keyboard_shortcut` appears in none of
`accesskit_windows`, `accesskit_macos` or `accesskit_atspi_common`, so the
default is discoverable only by triggering it. Where focus lands is the whole
contract.

Where the question is safe, put Yes back with `.default_button(StandardButton::Yes)`.
`Ok`, `OkCancel`, `SaveDiscardCancel` and `RetryIgnoreAbort` are unchanged: those
confirm something the user just asked for.

### Fixed (`teksilo-widgets`): a keystroke no longer recolours the whole document

Capturing a block's paint base on write left `apply_block_paint_spans` answering
`false` for a block that carries no overlay, correctly, since nothing changed.
But `apply_paint_highlights_for_range` read that answer as "this block could not
be recoloured" and reported a miss, and the rich-text frame loop treats a miss as
grounds to fall back to `flow_snapshot()` and recolour the whole document.

So an optimisation that removed a per-document copy put an O(document) pass back
on every keystroke that touched a block with no highlight on it, which is most of
them. A block that had nothing to clear now reports the no-op it performed.

### Added (`teksilo-widgets`): `WeakEditorHandle`, a handle that does not keep its editor alive

`on_image_activated`, `on_image_resized`, `on_link_activated`,
`on_files_dropped`, `on_change`, `on_text_inserted` and the image resolver are
all stored on the editor's own state. A handler that captured an `EditorHandle`
by value therefore made the state own itself, and nothing afterwards could break
the ring: the widget could be destroyed, its tree dropped and its window closed,
and the editor stayed resident with its document, its cursor and its shaped
layout.

Nothing about that is visible from the call site. Reaching for `editor.handle()`
is the obvious way for such a handler to act on the editor it belongs to. In a
downstream application that mounts one editor per scene, opening and closing a
single project leaked the whole manuscript, thirty-three documents at a time, on
a path where every widget, view-model and window had already been freed.

`EditorHandle::downgrade` gives those handlers a `WeakEditorHandle` to capture
instead, upgraded inside the handler. The handler then runs only while the editor
is alive, which is the only time it could have done anything anyway. A factory
the builder stores rather than the state, `context_menu` being the one today, may
still hold a strong handle.

### Fixed (`teksilo-core`): a closed window releases its event subscriptions

`TreeAppContext::subscription_callbacks` is one map shared by every window.
Entries were removed in exactly two places, both per-widget, in
`rebuild_single_widget` and `destroy_subtree_inner`. Closing a window runs
neither: `WindowManager::close_window` purged the context-bearing map and then
dropped the tree wholesale, with no per-widget destroy pass, and a plain entry
carried no window id, so it could not be purged even in principle. Every
`ctx.subscribe_event` a window's widgets ever made stayed in the map for the life
of the process, holding strong references to whatever the closure captured.

The plain map now stores `(Option<TeksiloWindowId>, callback)`, exactly as the
context-bearing map already did, recorded in `BuildContext::subscribe_event` from
the building widget's window, and `purge_subscriptions_for_window` is called
beside its context-bearing twin. A registration from a windowless tree records
`None` and is untouched by any window purge, so the headless and test paths keep
their existing per-widget teardown.

The shared map now holds `Rc` handles and dispatch clones the matching one out
before calling it, so a callback that drops the last handle to a window cannot
re-enter the map the dispatch is holding.

### Changed (`teksilo-text`): a font face is registered without copying its bytes

`FontFaceSpec::data` was `Arc<Vec<u8>>`, so a face compiled into the binary had
to be copied out of rodata onto the heap to be described, and
`FontRegistry::register_font` then copied it a second time. Both copies stayed
resident for the life of the process.

It now carries `text_typeset::SharedFontData`, which is
`Arc<dyn AsRef<[u8]> + Sync + Send>`, re-exported from `teksilo-text` because it
is the type of a public field. A `&'static [u8]` shares the rodata, an owned
buffer still coerces from `Arc<Vec<u8>>`, and `VecFontRegistrar` passes it to
`register_font_shared` rather than to the copying `register_font`.
`EmbeddedInterRegistrar` and `TypesetterBridge::register_default_font` took the
same two copies of Inter, JetBrains Mono and every optional script face, and now
take none.

This is a breaking change for code that constructs a `FontFaceSpec` directly.
`Arc<Vec<u8>>` still coerces, so most call sites compile unchanged; a site that
names the field's type has to be updated. In a downstream application that
registers eight bundled serifs, this returns 12.8 MB to the launcher's baseline.

### Fixed (`teksilo-charts`): a long category label no longer starves the plot

A tilted x-axis label's band grows with the widest label, about 0.707 times its
width at the auto 45 degrees, and `carve_plot_area` subtracted that band from the
chart's height with no floor under the result. A wide enough label therefore left
`plot.height == 0`, and both `BarChart::paint` and `LineChart::paint` return
early on a zero-height plot. The chart drew nothing at all: no bars, no grid, no
axis, no reference line, and no diagnostic anywhere. The widget still occupied
its box and still published a full set of per-datum accessibility marks, every
one of them zero pixels tall, so nothing downstream reported a problem either.

The labels now give way before the plot does. `MAX_X_LABEL_BAND_FRACTION` caps
the band at half the chart's height, and the cap is never applied below the
upright line height, so an axis whose labels already fit horizontally resolves
exactly as before. Capping the band alone would push the labels outside the
widget, over whatever sits beneath the chart, so `PlotGeometry` now carries the
band it actually granted and both charts size each label's text rect to it via
the new `axis::label_width_budget`. A label that no longer fits ends in an
ellipsis, which says there is more of the name where running off the edge says
nothing.

### Fixed (`teksilo-core`): observing a derived signal no longer panics

`Signal::observe` panicked on anything built with `map`, `zip`, `and` or `not`.
Every `Prop` in the framework accepts a derived signal, so the one consumer that
pushes instead of polling, the macOS native menu bridge, rejected exactly the
bindings the rest of the API invites. `enabled(unsaved.and(&backup_mode.not()))`
aborted the process before the first window was drawn: the panic unwinds through
`applicationDidFinishLaunching`, an Objective-C frame it cannot unwind through,
so it arrives as "panic in a function that cannot unwind".

`SignalKind::Derived` already kept its upstream mutable roots, but only for
generation polling, which serves the frame loop. An observer has no frame loop
behind it and has to be told, so `DerivedSource` now also carries a subscribe
hook and `try_observe` registers on every root, recomputing on any change. The
set is all-or-nothing on purpose: a partial subscription would report some
changes and silently miss the rest. `flat_map` re-selects its inner signal as it
is read and has no fixed root to attach to, so it still reports `ReadOnly`;
`map_coalesced` folds a fixed set and stays observable. `menu/native.rs` takes
the fallible path for that remaining case, leaving a row at the state it was
installed with instead of aborting the process.

### Fixed (`teksilo-platform`): the X11 drag-and-drop teardown is confirmed by the server

`DndThread::teardown` fired `DeleteProperty` and `DestroyWindow` and relied on
`flush()`. It is the last thing the DnD thread does, so the connection closes the
moment it returns, and a server that observes the disconnect before dispatching
the tail of that client's input queue drops the pending requests.

The proxy window disappears either way, since the disconnect frees the client's
own resources, but `XdndProxy` lives on the toplevel, which belongs to another
client. Losing that one request left the property pointing at a destroyed window,
and every later source that skips the specification's self-pointing validation
sent the whole handshake into it.

Both requests now go through `ignore_errors`, which checks rather than firing and
forgetting, so the server has processed the delete before the socket closes. The
cost is one exchange per window close. Reproduced in a container replicating the
CI job: 6 failures in 300 runs before, 0 in 600 after.

### Fixed (release): publish order accounts for dev-dependencies and the umbrella crate

`tools/check_release_order.py`'s topological sort read only `[dependencies]` and
`[build-dependencies]`, so a dev-only dependency on a later crate never affected
ordering, even though a default, verified `cargo publish` compiles dev-deps too.
Its internal-dependency filter also matched on a `teksilo-` prefix, silently
dropping any edge to the bare `teksilo` umbrella crate.

Together these put `teksilo-webview` and `teksilo-charts` before their
dev-dependency `teksilo-widgets`, and `teksilo-automation-mcp` before its real
dependency on `teksilo`, which is why three crates failed to publish mid-release
for 0.9.0.

## [0.9.0] - 2026-08-31

**Added**

- [The framework's own strings speak twenty-one more languages](#added--teksilo-widgets-the-frameworks-own-strings-speak-twenty-one-more-languages): nineteen
  European locales plus Japanese and Korean, each with its real CLDR plural
  categories rather than a copy of English's.
- [A runtime i18n override can watch a locale's whole directory](#added--teksilo-i18n-a-runtime-override-can-watch-a-locales-whole-directory), not
  just one `.ftl` file, so saving one resource no longer drops the keys its
  siblings define.
- [A number can be read back in the locale it was written in](#added--teksilo-i18n-a-number-can-be-read-back-in-the-locale-it-was-written-in).
  **Behaviour change:** `SpinBox` is `.localized(true)` by default, so a French
  user sees and types `12,5`.
- [The widget catalog renders its own pictures](#added--docs-the-widget-catalog-renders-its-own-pictures): 95 of 136 pages now
  open with a preview, rendered headless through the production wgpu renderer.
- [The framework answers "is the focused widget a text surface?"](#added--teksilo-core-the-framework-answers-is-the-focused-widget-a-text-surface), so an application owning a global `Ctrl+Z` can route it to whatever is
  being edited. New `WidgetTree::text_surfaces` and friends.
- [Cell editing with click triggers and persistent focus](#added--teksilo-widgets-cell-editing-with-click-triggers-and-persistent-focus): a per-column
  `EditTriggers` bitmask, dismissal on click-away, and `BuildContext::focus_into`.
- [A button icon may keep its own colour](#added--teksilo-widgets-a-button-icon-may-keep-its-own-colour) through `icon_keeps_color`,
  for a glyph whose colour is the information.

**Changed**

- [Overlay and tooltip bodies are built on first use](#changed--teksilo-core-overlay-and-tooltip-bodies-are-built-on-first-use) rather than on
  every rebuild of their owner. New `BuildContext::add_deferred`.

**Fixed**

- [A language switch reaches the date and time fields](#fixed--teksilo-widgets-a-language-switch-reaches-the-date-and-time-fields), which each read
  their locale convention once in `build()` and never again.
- [Escape reaches past a tooltip, and out of a text field](#fixed--teksilo-widgets-escape-reaches-past-a-tooltip-and-out-of-a-text-field): two
  independent swallows, both reachable from any dialog whose cancel sits outside
  a field.
- [A menu row activated by keyboard keeps the window sink](#fixed--teksilo-widgets-a-menu-row-activated-by-keyboard-keeps-the-window-sink) instead of
  panicking on `open_window`.
- [A drop into a folder no longer looks like a drop after it](#fixed--teksilo-widgets-a-drop-into-a-folder-no-longer-looks-like-a-drop-after-it): three
  verdicts had one visual.
- [The four Level-A findings of the internal accessibility assessment](#fixed--accessibility-the-four-level-a-findings-of-the-internal-assessment),
  including a keyboard trap in `Terminal` (WCAG 2.1.2).

### Added — `teksilo-widgets`: the framework's own strings speak twenty-one more languages

`teksilo-widgets` shipped its 308 user-facing strings — accessibility names,
MessageBox buttons, the GDPR privacy panel, calendar and keystroke names, the
command palette — in English and French only. Nineteen European locales plus
Japanese and Korean are now registered in `framework_locales()`: ar-SA, cs-CZ,
da-DK, de-DE, el-GR, es-ES, fi-FI, he-IL, hu-HU, it-IT, ja-JP, ko-KR, nb-NO,
nl-NL, pl-PL, pt-PT, ro-RO, ru-RU, sv-SE, tr-TR and uk-UA.

Each carries its real CLDR cardinal plural categories rather than a copy of
English's one/other shape, so a count-bearing message declines correctly where
the language demands it — one/few/many/other in the Slavic locales, one/few/other
in Romanian, the full zero/one/two/few/many/other in Arabic, and a bare other in
Japanese and Korean. Strings whose singular/plural split is chosen in Rust
rather than by Fluent were rephrased where a two-way split would be
ungrammatical at n = 2 or n = 5.

### Added — `teksilo-i18n`: a runtime override can watch a locale's whole directory

`runtime_override` accepted a single `.ftl` file, and a reload replaced the
locale's entire bundle with it. For a catalogue split across several files per
locale that is silently destructive: a bundle is the merge of every resource
registered for the locale, so saving `main.ftl` dropped every key
`tooltips.ftl` and its siblings defined, and those keys fell back to the source
locale with nothing printed anywhere. The translator sees most of the app
revert to English and has no reason to suspect the flag rather than their file.

`runtime_override` now takes a directory as well as a file, and the watcher
follows every resource under it.

### Added — `teksilo-i18n`: a number can be read back in the locale it was written in

`NumberFormatter` was a one-way street: it rendered a value for display and
offered nothing for the return trip, so an editable numeric surface could not
use it. Show a French user `1 234,56` and the commit path hands
`f64::from_str` a narrow no-break space and gets `None` — so every numeric
field was C-locale, and `SpinBox` showed `12.5` to a user whose numeric keypad
has no `.`.

`NumberSymbols` recovers a locale's separators, signs and digits *from ICU's
own formatted output*, so the display and parse directions cannot drift, and it
works in strings rather than `f64` so an `i64` past 2^53 keeps its precision.
`SpinBox` gained `.localized(bool)` (**default on**) and `.use_grouping(bool)`
(default off, matching Qt): display, commit parse and the per-character input
filter all resolve from one presentation, so they cannot disagree.

### Fixed — `teksilo-widgets`: a language switch reaches the date and time fields

Every datetime editing widget derives a display convention from the locale — a
strftime-subset pattern, a first day of week, a 12-vs-24-hour clock — and each
read it once, in `build()`. `set_locale` marks the tree dirty for layout and
paint and deliberately leaves `build()` alone, the same choice `set_theme`
makes, so nothing re-ran that read and a language switch left every date field
in the old convention.

### Added — docs: the widget catalog renders its own pictures

`extract_widget_api.py` has always emitted a preview image link on a catalog
page when the file exists, and nothing ever produced one. `teksilo-widgets-previewer
--export-docs` is the producer: it renders every registered widget through the
production wgpu renderer into `docs/widgets/img/`, headless, with no display
server. 95 of the 136 catalog pages now open with a preview. The images are
committed rather than built in CI, because the export needs a GPU adapter that
`ubuntu-latest` does not have.

### Added — `teksilo-core`: the framework answers "is the focused widget a text surface?"

An application that wants one Undo command routed to whatever the user is
actually editing must register `Ctrl+Z` globally — shortcuts resolve before any
widget sees the raw key — and the moment it does, it has taken that chord from
every text widget in the tree and owes each of them an answer. It cannot
produce one alone: it recognises the surfaces it built and is blind to the
rest, such as a rename box inside a table cell.

New API: `WidgetTree::text_surfaces()`, `focused_text_surface()` and
`focused_is_text_surface()`.

### Fixed — `teksilo-widgets`: Escape reaches past a tooltip, and out of a text field

Two independent bugs, both found chasing "Escape does nothing" in a table cell
editor. A *shown tooltip consumed Escape*: the dismissal path treats a
hover-shown overlay as a dismissal target and returns, so the key never reached
the focused widget — and a tooltip is up far more often than anyone realises.
And a focused `TextInputField` *swallowed* it: winit gives Escape
`text: Some("\u{1b}")`, `handle_key` had no `Escape` arm, so it fell into the
printable-character branch where the filter stripped the control character and
the empty result read as "input rejected". Every dialog whose cancel sits
outside a field had the same hole.

### Fixed — `teksilo-widgets`: a menu row activated by keyboard keeps the window sink

Enter on a menu item panicked the app with `open_window called on a standalone
WidgetTree (no app context)`, while the same item opened its window fine by
mouse. A keyboard activation does not dispatch the click a pointer would: the
menu queues a synthetic click and the tree drained it through
`WidgetTree::click` — the *test* entry point, which installs `NoopWindowOps`.

### Added — `teksilo-widgets`: cell editing with click triggers and persistent focus

Cell editors now activate on single or double click, configurable per column
through the new `EditTriggers` bitmask, and `on_cell_edit_dismissed` ends an
active edit when the user clicks away — on another cell or on empty table
space. In tree tables, keyboard focus now moves into the editor and *stays*
there across rebuilds, via the new `BuildContext::focus_into`. Tap ownership in
the dispatcher was refined so double-click-to-edit no longer costs the row
selection on the first click.

### Added — `teksilo-widgets`: a button icon may keep its own colour

`Button` binds every icon's tint to the label's colour, which is right for a
glyph that repeats the label and wrong for one whose colour *is* the
information: a filter chip carrying a user-chosen tag colour, a legend swatch,
a status disc. `icon_keeps_color` is the same opt-out `MenuItem` has had, on
the same reasoning, so a tag reads the same in a chip as in the menu that filed
it.

### Changed — `teksilo-core`: overlay and tooltip bodies are built on first use

A widget owning overlay content — a popover's panel, a dropdown, a submenu, a
calendar, a tooltip body — wrote `ctx.add(panel)` followed by
`ctx.set_dormant(id)`. That is correct, and it is what dormancy documents:
parking is about activation, not construction. What it costs is a full
`build()` of content the user may never open, on every rebuild of the owner —
invisible in a dialog, dominant in a virtualized collection where the owner is
a per-row delegate.

New API: `BuildContext::add_deferred` / `add_deferred_boxed` and
`DeferredSubtree`, which materializes its child on first reveal.

### Fixed — `teksilo-widgets`: a drop into a folder no longer looks like a drop after it

Dragging a row over a container offered three verdicts and one visual. The
`Into` box was painted at the target row's exact bounds, so its top edge
occupied the pixels a `Before` line does and its bottom edge those of an
`After` line; its wash was alpha 0.18 of the accent, invisible on light chrome;
and the drag ghost covers the row's right half, hiding the vertical sides that
were the only thing saying "box" rather than "line". Whichever third of the row
you hovered, you read an accent bar at a row boundary.

### Fixed — accessibility: the four Level-A findings of the internal assessment

The 2026-08-28 internal assessment raised four Level-A defects in four
different crates. They share a shape: the toolkit advertised a capability it
did not provide. The clearest is a keyboard trap (WCAG 2.1.2) — `Terminal` sets
`keyboard_capture(true)` and answered `Handled` to every `KeyDown` after
encoding Tab and Shift+Tab for the child, so no chord could leave the widget.

The audit document itself was re-checked against source and reframed as an
internal engineering assessment — not an ACR, not a VPAT, not a conformance
claim, not third-party verified — with a scope-and-method section stating what
was *not* done: no live AT testing, no colorimetry, some widgets unassessed.

## [0.8.0] - 2026-08-27

**Added**

- [The macOS (Aqua / Dark Aqua) preset](#added--teksilo-theme-macos-the-macos-aqua--dark-aqua-preset), a complete design language
  behind the `theme-macos` feature, replacing a stub.
- [The Fluent (Windows 11 / WinUI 3) preset](#added--teksilo-theme-fluent-the-fluent-windows-11--winui-3-preset), likewise, behind
  `theme-fluent`, transcribed from WinUI's own theme dictionaries.
- [`Settings…` in the macOS App menu](#added--teksilo-widgets-settings-in-the-macos-app-menu) via
  `StandardMenu::settings_intent`, a placement an ordinary `MenuEntry` cannot
  reach.
- [The automation `scroll` op carries modifiers](#added--teksilo-automation-scroll-carries-modifiers), so a probe can perform
  Ctrl+wheel and not only describe it.
- [Two defaulted label-role style hooks](#added--teksilo-core-two-defaulted-label-role-style-hooks) so a design language with a
  solid selection fill can recolour the text on top of it.

**Changed**

- **A declared `Ctrl` shortcut now fires on ⌘ on macOS.**
  **Behaviour change on macOS:** it no longer fires on physical ⌃. Opt out per
  chord with the new `ShortcutBuilder::literal_modifiers()`.
- [macOS in the widget catalog's theme switcher](#changed--widget-catalog-macos-in-the-theme-switcher-and---theme) and `--theme`.
- [Fluent in the widget catalog's theme switcher](#changed--widget-catalog-fluent-in-the-theme-switcher-and---theme) and `--theme`.

**Fixed**

- [Caret motion follows the platform's own layout](#fixed--teksilo-widgets-caret-motion-follows-the-platforms-own-layout): macOS spreads word,
  line edge and document across three modifiers, and every text surface read a
  single accelerator flag.
- [Accelerator chords across the widget catalog](#fixed--teksilo-widgets-accelerator-chords-across-the-widget-catalog): select-all, the
  discontiguous-selection click, the marquee modifier and Ctrl+Home / End all
  tested physical Control.
- [A tree row's chevron ignored its row's colour](#fixed--teksilo-widgets-a-tree-rows-chevron-ignored-its-rows-colour), leaving it at roughly
  2.5:1 under a style that flips a selected row's label.
- [The `SpinBox` mouse wheel was inverted](#fixed--teksilo-widgets-spinbox-the-mouse-wheel-was-inverted) against every other spin box
  and against its own ArrowDown key.

### Fixed — `teksilo-widgets`: caret motion follows the platform's own layout

Word-jump, the line edge and the document edge do not merely sit on a different
modifier on macOS — they are laid out differently. Windows and Linux put word
on `Ctrl+←/→` and the line edges on bare `Home`/`End`; macOS spreads the same
motions across three modifiers on the arrows themselves: `⌥←/→` word, `⌘←/→`
line edge, `⌘↑/↓` document, `⌥↑/↓` paragraph.

Every text surface read a single "is the accelerator held?" flag, so on macOS
`⌘←` jumped a word (it should reach the start of the line), `⌘↑` moved one line
(it should reach the top of the document), and `⌥←` did nothing at all. Reading
the chords through the new `common::text_nav` fixes `RichTextEditor`,
`CodeEditor`, and `TextInput` / `SearchField` / `PasswordField` / `SpinBox` /
`DateEdit` through the shared `TextInputField`. Word-delete moves with them —
`⌥⌫` on macOS, `Ctrl+⌫` elsewhere. `LogView` is untouched: it scrolls a
read-only view and has no caret to move.

Windows and Linux are unchanged. `Alt+↑/↓` keeps move-line in the code editor
on every platform (the binding every code editor ships, macOS included), so
only the rich-text editor reads `⌥↑/↓` as a paragraph motion. `⌘⌫` means
delete-to-line-start on macOS, which is not implemented; it falls through to a
plain single-character delete rather than removing more than was asked for.

`⌘←/→` folds through the caret's text direction the same way character and word
steps already do, so all three horizontal motions agree about which way "left"
is inside an Arabic paragraph.


### Changed — `teksilo-core`: a declared `Ctrl` shortcut now fires on ⌘ on macOS

**Behaviour change on macOS.** A `Shortcut` whose declared default is a `Ctrl`
chord is now read as *the platform's primary accelerator* and resolves to ⌘
there — the convention Qt spells `Qt::CTRL`, and the one Teksilo's native menu
bar has always applied when building `NSMenuItem` key equivalents. So
`Shortcut::new("editor.find").primary(KeyStroke::ctrl(Key::F))` fires on ⌘F on
macOS and on Ctrl+F everywhere else, from one declaration.

Until now the registry matched chords by exact modifier equality while the
native menu row advertised ⌘, so on macOS a command that appeared on a menu
worked (AppKit dispatched the key equivalent) and an identical command that did
not appear on one silently required physical ⌃. Three places in the framework
answered the Ctrl-vs-Command question independently and the one that dispatches
keystrokes was the one that did not.

The corollary is that a declared `Ctrl` chord **no longer fires on physical ⌃**
on macOS, where Control belongs to the text system and to the secondary click.

**If you have a chord that really is Control on every platform** — Ctrl+Tab
cycles tabs on macOS too, and the ⌘ form of Space, H or Q is taken by the
system — declare it with the new opt-out:

```rust
Shortcut::new("view.next_tab")
    .literal_modifiers()
    .primary(KeyStroke::ctrl(Key::Tab))
    .build()
```

Worth auditing your declarations for that shape; nothing else needs changing.
User overrides are deliberately left literal, so a chord captured in a settings
UI still means exactly the keys that were pressed and physical ⌃F stays
bindable on macOS.

New API: `Modifiers::COMMAND` / `Modifiers::command()` (the platform's primary
accelerator, for widgets testing a live `Modifiers`), `KeyStroke::command()` /
`command_shift()` (the same intent stated outright, for chords built outside the
registry), `KeyStroke::with_command_convention()`, `Shortcut::declared_keystrokes()`,
`ShortcutBuilder::literal_modifiers()`.

### Fixed — `teksilo-widgets`: accelerator chords across the widget catalog

The same gap, widget-side. Select-all, the discontiguous-selection click, the
marquee's additive modifier and Ctrl+Home / Ctrl+End were all testing physical
Control, so on macOS ⌘A did not select all and ⌘-click did not extend a
selection — while ⌃-click, which macOS treats as the secondary click, did.
`ListView`, `TreeView`, `TableView`, `TreeTableView`, `GridView`, `Calendar` and
the colour picker's swatch grid now test `Modifiers::command()` for those.

The text surfaces (`RichTextEditor`, `TextInputField` and everything built on
it, `CodeEditor`, `LogView`) tested `ctrl() || super_key()`, which worked on
macOS but also made the Win key act as Ctrl on Windows and Linux; they now test
the accelerator too. The Explorer-style cursor pair (Ctrl+Space / Ctrl+Arrow),
Ctrl+Tab's focus escape, Ctrl+Space forced completion and the terminal's control
codes deliberately stay on literal Control — each says so at its site.

Context-menu labels are fixed with them: the built-in Cut / Copy / Paste /
Select All rows built their accelerator text from hard-coded `Ctrl` literals, so
on macOS a right-click in any text field read "Copy ⌃C" while the menu bar
showed ⌘C for the same command.

### Added — `teksilo-widgets`: `Settings…` in the macOS App menu

`StandardMenu::settings_intent("app.settings")` puts a **Settings…** row in the
application menu, under About, on ⌘, — placement and chord that an ordinary
`MenuEntry` cannot reach, since the App menu is filled in by the platform.
Pair it with `.settings(tr!(settings()))` for the label.

Unlike Quit there is no system fallback — no platform opens an arbitrary app's
settings on its own — so leaving `settings_intent` unset omits the row rather
than rendering a dead one.


### Added — `teksilo-automation`: `scroll` carries modifiers

The `scroll` op hardcoded `Modifiers::NONE`, so it could only ever deliver a
bare wheel. It now takes `ctrl` / `shift` / `alt` / `meta` alongside `dx` / `dy`,
mirroring `inject_key`; all default to false, so every existing caller is
unchanged.

A modifier-held wheel is a distinct gesture, not a decorated one —
`WidgetEvent::Scroll` carries a `modifiers` field precisely so an app can
implement Ctrl-wheel-to-zoom, and its own doc comment says as much. Until now
that was the one input the bridge could *describe* but not *perform*: a probe
could assert that a plain wheel scrolls and never that Ctrl+wheel does anything
else, which is exactly backwards for a feature whose whole risk is the two being
confused. Two tests pin it — that each modifier reaches the widget as asked, and
that the no-modifier default is still a plain wheel.

### Added — `teksilo-theme-macos`: the macOS (Aqua / Dark Aqua) preset

`teksilo-theme-macos` was a stub returning an IntUI-shaped baseline behind a
wall of `TODO(macos)` markers. It is now a complete design language, opt-in via
the umbrella crate's `theme-macos` feature and reachable as
`teksilo::prelude::macos::{light, dark}`.

**Tokens, with their provenance stated.** Apple attaches a standing disclaimer
to every colour value it publishes ("the actual color values will fluctuate from
release to release"), and publishes no table at all for corner radii, control
heights, focus-ring geometry or animation durations bar one. So every literal in
the crate is tagged at its definition: **`[HIG]`** (published — the 13-hue system
colour table and the whole typography ramp), **`[measured]`** (a community
capture of the private `NSColor` enumeration, or a screen measurement), or
**`[derived]`** (computed from one of the first two, with the rule given). The
full AppKit vocabulary — four label grades, two independent selection families,
the bezel description, the eight System Settings accents — is exposed through
the **`MacOsPalette`** theme extension.

Geometry is the two measured radii (6 dp in-page, 10 dp floating, with a menu at
its own measured 9 dp) and a **22 dp** control height — a third shorter than
Fluent's 32, which is most of why a macOS window fits more. Typography is the
published text-style ramp (Body 13/16, Callout 12/15, Subheadline 11/14) with
Apple's **signed tracking**: −0.08 at 13 pt, exactly 0 at 12, +0.06 at 11. It is
the only Teksilo preset that tracks non-uniformly, and the only one whose
tracking changes sign. Motion is Core Animation's default 0.25 s on
`kCAMediaTimingFunctionEaseInEaseOut`, `cubic-bezier(0.42, 0, 0.58, 1)` —
symmetric, where Fluent's curve is decelerate-only.

**Chrome for 28 style slots.** Eight are real `impl FooStyle` blocks, for the
controls where AppKit is structurally its own thing:

- the push button's **bezel** — a soft shadow, a faint top-to-bottom face
  gradient, a hairline, and (in Dark Aqua) a catch-light along the top edge.
  Pressing it darkens the face and drops the shadow, so the control settles into
  the surface. The *default* button deliberately gets none of it: a flat accent
  fill, because doubling the elevation cues is what makes a macOS default button
  look wrong;
- a focus ring that **is the accent** — not Fluent's high-contrast neutral
  outline. Drawn naively that fails WCAG SC 1.4.11 (the accent at the ~50 %
  alpha the halo appears to have measures 1.86:1 on `windowBackgroundColor`), so
  it is built as two bands: a 2 dp solid one at full accent that carries the
  contrast, and a translucent halo outside it that supplies the macOS softness;
- the `NSSwitch`'s **18 dp knob in a 22 dp track** — a knob that nearly fills its
  corridor, wearing the same bezel the push button does, where Fluent's is a
  12 dp dot in a 20 dp track;
- the 14 dp checkbox and radio (half the size of Fluent's 20 dp box), bezelled
  while unchecked, with AppKit's forward-leaning tick and its small fixed pip;
- the field's **accent focus halo** — the whole announcement, where Fluent grows
  its bottom edge and Material 3 thickens its outline;
- the slider's plain round bezelled knob, which unlike Fluent's does not resize
  under the pointer;
- the menu row's **accent fill with a white label**, where Fluent uses a neutral
  wash;
- the list row's **selection capsule** — Big Sur's inset rounded rectangle,
  accent-filled with `alternateSelectedControlTextColor` on top.

The other twenty are the shipped `Recipe*Style` constructed with AppKit metrics,
not reimplementations.

**Accent.** `controlAccentColor` is whatever the user picked in System Settings.
`light()` / `dark()` resolve it against the out-of-box `systemBlue`;
**`light_with_accent(Color)` / `dark_with_accent(Color)`** rebuild the accent
family around any other seed, and **`SystemAccent`** carries the eight swatches
macOS itself offers. `linkColor` deliberately does *not* follow — AppKit keeps
links a fixed blue whatever accent is chosen. Note that an accent-filled control
paints `selectedContentBackgroundColor` (a *darkened* accent) rather than the raw
one: white on the raw `#007AFF` is only 4.02:1, so painting it would have shipped
an inaccessible default button.

**Four documented deviations from Apple's own numbers**, each with the
measurement that forced it and a test that pins the premise:

- **The label grades are lifted, and the lift is computed rather than
  guessed.** `secondaryLabelColor` is 50 % black in Aqua (3.98:1 on
  `textBackgroundColor`) and 55 % white in Dark Aqua; neither clears WCAG SC
  1.4.3's 4.5:1 floor on every surface the preset paints. Rather than hand-pick
  a number per appearance, the projection raises the alpha until it clears the
  floor on each surface **including the hover and press washes over it** — a
  press wash moves a surface *toward* the label, and that is where Apple's own
  value first stops passing. Apple's numbers stay on `MacOsPalette`; the lifted
  ones are what `ColorTokens` carries.
- A 45 % `border_strong` where AppKit's control hairline measures 1.25:1
  against WCAG 1.4.11's 3:1 floor for a control boundary.
- The *Accessible* variant of each system colour for status foregrounds rather
  than the Default one (`systemRed` Default is 3.55:1 on white).
- The IntUI amber kept for the search highlight, because `findHighlightColor` is
  pure `#FFFF00` in both appearances and AppKit special-cases the text on it to
  black.

A discrete slider's **tick marks** paint `border_strong` rather than
`tertiaryLabelColor` for the same reason a checkbox outline does — a tick marks
a value the slider can actually take, and AppKit's 25 % tertiary label measures
1.8:1 on the window in Aqua. The slider's *rail* is deliberately left faint:
there the knob is the control and the rail is the groove behind it, which AppKit
also keeps subtle. Both are commented at the paint site so the asymmetry is not
mistaken for an oversight.

**Known limitations, stated rather than deferred.** The OS accent is not read —
Teksilo's platform layer returns only the light/dark preference on macOS, so
there is no live `NSColor.controlAccentColor` to bind to. Vibrancy uses each
material's opaque fallback, which is what macOS itself shows with "Reduce
transparency" on. The `TableView` / `GridView` selection band is an accent *wash*
rather than the capsule, because those views paint the shared `surface_selected`
token behind app-supplied cell widgets this preset cannot retint. San Francisco
cannot be bundled, so the optional `system-fonts` feature *names* the macOS faces
and the default build keeps the metric-neutral bundled Inter.

### Added — `teksilo-core`: two defaulted label-role style hooks

`StandardItemStyle::selected_label_role` and
`MenuItemStyle::highlighted_label_role`, both defaulting to `None`.

A row builds its label before any style's `make_body` runs, so a design language
whose selection is a **solid** fill could not recolour the text on top of it —
macOS's accent capsule would have left `labelColor` at roughly 3.5:1. These
declare the role instead, and the widget composes it into the label's (and the
menu row's shortcut's) colour signal, gated on the row actually being emphasised
so an unemphasised or window-inactive row keeps its normal label. Same shape as
the existing `ButtonStyle::label_text_role`, and defaulted for the same reason:
IntUI and Fluent use pale washes, need no flip, and are unchanged.

### Fixed — `teksilo-widgets`: a tree row's chevron ignored its row's colour

`TwistArrow` painted a hardcoded `TextRole::Secondary` with no way to override
it. That was invisible while every theme's selection was a pale wash, and wrong
the moment one is not: under a style that flips a selected row's label (see
above), the label turned white and the chevron stayed a grey smudge on the
accent capsule — roughly 2.5:1, under WCAG SC 1.4.11's 3:1 floor. `MenuItem`'s
own chevron was already wired to its row's text role; `StandardTreeItem`'s was
not.

`TwistArrow::color(impl Into<ColorProp>)` now takes any colour, role or signal
(defaulting to the previous `Secondary`), and `StandardTreeItem` hands it the
same role its label uses, so the two always move together. Under IntUI and
Fluent nothing changes.

### Changed — `widget-catalog`: macOS in the theme switcher and `--theme`

The catalog's title-bar `ThemeSwitcher` gains macOS Light / macOS Dark, and
`--theme` accepts `macos-light` / `macos-dark`. Both are covered by the existing
persist/restore round-trip test, the failure mode where a new theme persists
happily and then silently reverts on the next launch.

### Fixed — `teksilo-widgets` SpinBox: the mouse wheel was inverted

Scrolling **down** over a `SpinBox` increased its value, and scrolling up
decreased it — the opposite of `QAbstractSpinBox`, `GtkSpinButton` and WinUI's
`NumberBox`, and the opposite of the widget's own ArrowDown key. Independent of
the theme.

`ScrollDelta` is not winit's raw wheel reading: `translate_mouse_wheel` negates
it so that **positive y increases a scroll offset** — which is why `ScrollArea`,
every data view and the `TabBar`'s wheel-to-horizontal remap all add it straight
to their scroll position. `SpinBox` was the one consumer mapping a wheel notch
to a *value* rather than an offset, and it read positive y as "the user scrolled
up". Now `y > 0` steps down. The wheel path had no test coverage at all; it has
three now, including one that pins the wheel against the arrow keys.

### Added — `teksilo-theme-fluent`: the Fluent (Windows 11 / WinUI 3) preset

`teksilo-theme-fluent` was a stub returning an IntUI-shaped baseline behind a
wall of `TODO(fluent)` markers. It is now a complete design language, opt-in via
the umbrella crate's `theme-fluent` feature and reachable as
`teksilo::prelude::fluent::{light, dark}`.

**Tokens, transcribed from primary source.** Every colour comes from WinUI's own
`Common_themeresources_any.xaml`, written in WinUI's `#AARRGGBB` notation so the
file diffs against the theme dictionary line by line. The full token set —
including the graded control fills, the on-accent strokes and the system fills
that Teksilo's `ColorTokens` has no slot for — is exposed through the
**`FluentPalette`** theme extension. Geometry is Fluent's two radii
(`ControlCornerRadius` 4 dp for in-page and bar elements, `OverlayCornerRadius`
8 dp for anything that floats, with the tooltip as the documented 4 dp
exception). Typography is the WinUI type ramp — Body 14/20, Caption 12/16 — at
**zero tracking** on every rung, the opposite of Material 3. Motion is the four
`Control*AnimationDuration` steps on `ControlFastOutSlowInKeySpline`,
`cubic-bezier(0, 0, 0, 1)`.

**Chrome for 25 style slots.** Eight are real `impl FooStyle` blocks, for the
controls where WinUI is structurally its own thing:

- the button's **elevation edge** — a heavier stroke along the bottom edge in
  light (a cast shadow) and the top edge in dark (a catch-light), dropped on
  press. WinUI produces it with a gradient border brush anchored to a fixed 3 dp
  band, flipped in the light dictionary only;
- a **two-tone, high-contrast focus ring** — 2 dp near-black (light) or white
  (dark) outside a 1 dp opposite-colour inner ring. Fluent never tints the focus
  indicator with the accent, so neither does this;
- the `ToggleSwitch`'s off-state outline, grey knob, and knob that grows on
  hover and squashes to 17 × 14 under the press;
- the filled unchecked checkbox and radio (an `ControlAltFill` box inside a
  `ControlStrongStroke` outline — IntUI's transparent-with-a-hairline box would
  fail WCAG 1.4.11 on a light Fluent surface);
- the field's **accent focus underline** — `TextControlBorderThemeThicknessFocused`
  is literally `1,1,1,2`;
- the slider's two-circle thumb, whose accent inner dot swells to 14 dp on hover
  and shrinks to 10 dp while dragging;
- the menu row's **neutral** hover (`SubtleFillColorSecondary`, not an accent
  tint);
- the list row's **selection pill** — the 3 × 16 dp accent bar on the leading
  edge that makes Windows 11's neutral selection wash legible.

The other seventeen are the shipped `Recipe*Style` constructed with Fluent
metrics, not reimplementations.

**Accent.** `AccentFillColor*` is not a literal in WinUI — it binds to the ramp
Windows generates from the user's accent, and the two appearances pull from
*opposite ends* of it (light fills with a darkened accent and white labels, dark
with a lightened one and black labels). `light()` / `dark()` resolve it against
the Windows out-of-box `#0078D4`; **`light_with_accent(Color)` /
`dark_with_accent(Color)`** rebuild the whole accent family around any other
seed, leaving every neutral token untouched.

**Known limitations, stated rather than deferred.** Mica and Acrylic are
compositor materials with no flat-fill equivalent; every surface uses the opaque
fallback WinUI itself falls back to when the material is unavailable. Segoe UI
Variable is proprietary and cannot be bundled, so the optional `system-fonts`
feature *names* the Windows faces for the text engine to resolve and the default
build keeps the metric-neutral bundled Inter.

### Changed — `widget-catalog`: Fluent in the theme switcher and `--theme`

The catalog's title-bar `ThemeSwitcher` gains Fluent Light / Fluent Dark, and
`--theme` accepts `fluent-light` / `fluent-dark`. Theme restore-on-launch moved
from an inline `match` to a named `theme_from_id`, and the example gained its
first tests — including one that walks every offered preset through a
persist/restore round trip, the failure mode where a new theme persists happily
and then silently reverts on the next launch.

## [0.7.0] - 2026-08-08

**Added**

- [Docking rail actions and Strip bar slots](#added--teksilo-widgets-docking-rail-actions--strip-bar-slots): `DockAction` puts a plain
  command in the activity rail, and `DockRail::leading_slot` / `trailing_slot`
  reach a side showing its tab strip.
- [`ListModel::reconcile_by_key`](#added--teksilo-data), which diffs against a new
  authoritative `Vec<T>` and emits minimal granular changes rather than a blanket
  `Reset` that would clear the user's selection.

**Changed**

- [`teksilo-settings` is cross-process safe by default](#changed--teksilo-settings-breaking), not by opt-in.
  **Breaking:** `SettingsFile::load_shared`, `MruList::toggle_pin` and
  `PersistedTreeModel<T>` are removed, `load` / `load_strict` dropped their
  `delay` parameter, and `MruEntry` now requires a separate `Keyed` trait.
- [`NotificationArchiveModel::remove(index)` is now `remove_by_id(id)`](#changed--teksilo-widgets-breaking).
  **Breaking:** an index names a position a peer's insert can invalidate.

**Fixed**

- [The docking activity rail was an invalid ARIA `tablist`](#fixed--teksilo-widgets-docking-rail-accessibility): the slots,
  overflow trigger and action clusters were non-`Role::Tab` children of it. No
  app-facing API changed.

### Added — `teksilo-widgets` docking: rail actions + Strip bar slots

**`DockAction` — dockless command buttons in the activity rail.** A rail item
was previously always one activity (a tab with a panel behind it); there was no
way to put a plain command in that column. `DockRail::action(DockAction::new(…))`
adds one, rendered by the framework so it matches a real activity item — same
size modes, same selected-surface highlight, same side-placed tooltip, same
`Icon + Label` rotated caption.

An action is deliberately more restricted than an activity: never draggable,
never hidable, no "Move to" menu, and never overflow-parked (it is reserved
space). `DockActionPlacement::{Start, End, Pinned}` picks the cluster — `Pinned`
sits past the spacer at the rail's far edge, VS Code's Accounts / Manage
position, where a Settings gear belongs. `DockActionId::named("…")` is a
`const fn` (FNV-1a), so ids are module-scope `const` items and stay stable across
runs for the accessibility tree and the automation bridge.

Nothing about an action is persisted — it carries no user-mutable state, so it
is app config reconstructed each run, like the rail's slots.
**`DockLayoutState` is unchanged and needs no migration.** `DockPolicy` is
unchanged too: with hiding off the table there is nothing to gate, and a flag
that enforced nothing would lie.

`DockAction::toggled(signal)` is **reflect-only** — the rail never writes it, so
a derived signal is safe (unlike `IconButton::toggle`, which flips its signal on
click). `on_activate` owns every write.

**`DockRail::leading_slot` / `trailing_slot` — bar slots for a Strip side.**
`top_slot` / `bottom_slot` only ever reached the Rail presentation; a side
showing its in-side tab strip had no app-facing slot at all, even though
`TabWidget` has had `bar_leading_slot` / `bar_trailing_slot` all along. Two
things had to be fixed for that to be usable rather than a footgun:

- `TabWidget`'s bar slot is a single last-write-wins field, and `DockSidePanel`
  already spends the trailing one on its "hidden activities" hamburger — the
  only way back once every activity on a side is hidden. The two are now
  explicitly composed, so declaring a trailing slot can no longer silently drop
  the hamburger (or vice versa).
- A side with **zero** docks returned early, before its `TabWidget` was built,
  so a configured slot silently rendered nothing — a reachable state, not
  misuse. (The same bug Qt's `QTabWidget::setCornerWidget` still has: the corner
  widget only shows while at least one tab exists.) Slots now render there too.

These carry a weaker visibility contract than the rail slots and it is
documented as such: the activity bar is built whenever the side has a rail, so
`top_slot` / `bottom_slot` survive a collapsed side, while `leading_slot` /
`trailing_slot` live inside the collapsing content region and disappear with it.

### Fixed — `teksilo-widgets` docking: rail accessibility

**The activity rail was an invalid ARIA `tablist`.** `DockActivityBar` set
`Role::TabList` on its whole root, so `top_slot` / `bottom_slot` widgets and the
overflow trigger were non-`Role::Tab` children of a tablist — which ARIA's Tabs
pattern forbids (Required Owned Elements), and which real screen readers
navigate poorly. The role now sits on a `DockRailTabList` wrapping **only** the
items; the slots, the overflow trigger and the new action clusters are siblings,
and the rail root is a property-free `Role::GenericContainer` that the AT pass
prunes. Restricting the AT children instead (via `accessibility_children`) would
have deleted those controls from the tree rather than re-parenting them, turning
a spec violation into a WCAG 2.1.1 failure — hence the wrapper.

No app-facing API changed for this; a rail still reports one `Role::TabList` per
side with the same localized name.

### Changed — `teksilo-settings` (breaking)

**Cross-process safety is now the default and only behaviour for every
persisted type in the crate**, not an opt-in mode. Previously,
`docs/settings.md` said — twice, deliberately — "multi-process is out of
scope; two app instances writing to the same file is last-write-wins," then
later grew an opt-in `SettingsFile::load_shared` escape hatch for callers
that needed better. Both of those are gone: `SettingsStore`,
`SettingsFile<T>`, and `PersistedListModel<T>` (and everything built on
them — `MruList<T>`, `WindowStateService`) now always merge a write against
the document read fresh under an exclusive lock, and a peer process's write
now arrives live, through the same `Signal`/`ListModel` a caller is already
bound to, via a new `notify`-based file watcher — with nothing for the
caller to remember to call, unlike Qt's `QSettings::sync()`. See
`docs/settings.md` for the full architecture (the patch/merge model, why a
lock alone isn't sufficient, the live-reload watcher, and the honest
performance trade-offs).

- **Removed:** `SettingsFile::load_shared`. `SettingsFile::load` /
  `load_strict` are unconditionally correct now — there is no longer a
  "which mode did I open this with" question to get wrong. `load` /
  `load_strict` also dropped their now-meaningless `delay: Duration`
  parameter (`SettingsFile<T>`'s writes are always synchronous).
- **Removed:** `MruList::toggle_pin`. Replaced by
  `MruList::set_pinned(key, bool)`. A toggle's effect depends on the
  *current* state at the moment it runs, which is exactly the kind of
  transient, position-dependent meaning a replayable, possibly-delayed
  write can no longer assume — `set_pinned` states the desired end state
  directly, so replaying it against a document a peer has already changed
  always lands on the same answer.
- **Removed:** `PersistedTreeModel<T>` (and its `collection::tree` module).
  Zero consumers anywhere in this workspace or in Skribisto, and it carried
  the exact whole-snapshot-clobber defect the rest of this crate was just
  hardened against. Reintroduce it ops-based, from scratch, if a real
  consumer needs a persisted tree — do not resurrect the deleted version.
- **Changed (source-compatible):** `Keyed` is a new, separate trait
  (`type Key` + `fn key(&self) -> Self::Key`, owned key) that `MruEntry`
  now requires alongside its existing pin/touch methods; existing
  `impl MruEntry` blocks need a companion `impl Keyed` (previously `Key`
  and `key()` lived directly on `MruEntry`).
- **Source-compatible, despite the headline:** `SettingsFile::mutate`,
  every `SettingsStore` signal (`signal`/`signal_for`/`.set()`), and
  `MruList::add`/`touch`/`remove`/`clear` keep their existing call-site
  shape. "Cross-process safe by default" sounds like it should cost
  callers something; it doesn't — the correctness moved into the write
  path and the new live-reload watcher, not into new ceremony at the
  call site.

### Changed — `teksilo-widgets` (breaking)

- `NotificationArchiveModel::remove(index: usize)` → `remove_by_id(id: u64)`.
  Same reasoning as `set_pinned` above: an index names a position in this
  process's *current* view of the list, which a peer's concurrent insert
  (or this crate's own debounce delay) can invalidate before the removal
  actually runs. An id is stable identity regardless of how many
  neighboring rows moved in the meantime.

### Added — `teksilo-data`

- `ListModel::reconcile_by_key(new_items, key_fn)`: diffs the live model
  against a new authoritative `Vec<T>` by key and emits the minimal
  granular `DataChange`s (coalesced removes/inserts, single-row moves only
  where something is actually out of place, updates where content
  differs) — and **never** a blanket `Reset`, which would otherwise clear
  a user's positional selection every time a peer's settings write landed.
  This is the primitive `PersistedListModel<T>`'s live-reload path is
  built on.
- `adjust_single_index_for_change` (in `data_change.rs`) and its use in
  `ListView`'s focused-index tracking, so a single-selection widget keeps
  its focus pointed at the right row across a reload-driven reconciliation,
  not just across ordinary user-driven inserts/removes.

## Earlier history

Entries before this file was introduced are not backfilled; see `git log`
for the full history.

[Unreleased]: https://github.com/FernTech-EU/teksilo/compare/v0.9.2...HEAD
[0.9.2]: https://github.com/FernTech-EU/teksilo/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/FernTech-EU/teksilo/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/FernTech-EU/teksilo/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/FernTech-EU/teksilo/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/FernTech-EU/teksilo/compare/v0.6.2...v0.7.0
