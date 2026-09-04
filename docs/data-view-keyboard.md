<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Data-view keyboard navigation

The five data views — [`ListView`](../crates/teksilo-widgets/src/list_view.rs),
[`TreeView`](../crates/teksilo-widgets/src/tree_view.rs),
[`TableView`](../crates/teksilo-widgets/src/table_view.rs),
[`TreeTableView`](../crates/teksilo-widgets/src/tree_table_view.rs) and
[`GridView`](../crates/teksilo-widgets/src/grid_view.rs) — answer one keyboard
contract. This page is that contract, plus the two places Teksilo knowingly
departs from a platform and why.

The chord table itself lives in one module,
[`common/list_nav.rs`](../crates/teksilo-widgets/src/common/list_nav.rs), as
pure functions over `(key, modifiers, view kind)`. Each view keeps only its own
index arithmetic. Before that module the five views had three different answers
for `Home`.

## The bindings

`Linear` = ListView, TreeView. `TileGrid` = GridView. `CellGrid` = TableView /
TreeTableView in a cell-selection mode; in a row-selection mode those two read
as `Linear`.

| Chord | Linear | TileGrid | CellGrid |
|---|---|---|---|
| `Home` / `End` | first / last item | first / last item | first / last cell **of the row** |
| `Ctrl`+`Home` / `End` | the same, selection untouched | the same, selection untouched | the corner (`TableView`) or the same column (`TreeTableView`), selection untouched |
| `PageUp` / `PageDown` | ± one viewport of rows | ± one viewport of rows | ± one viewport of rows, column kept |
| `Shift`+*any of the above* | extend from the anchor | " | " |
| `Ctrl`+`Shift`+*any* | extend **additively** | " | " |
| `Ctrl`+`↑`/`↓` | move the cursor only | move the cursor only | move the cursor only |
| `Space` | **check the row** if it has a checkbox, else toggle (Multi) / select (Single) | toggle (Multi) / select (Single) | toggle the cell |
| `Ctrl`+`Space` | toggle the focused row | toggle the focused tile | **select the column** (`MultiCell`) |
| `Shift`+`Space` | — | — | **select the row** (`MultiCell`) |
| `Enter` | activate | activate | activate the row |
| `Ctrl`+`A` / `Ctrl`+`Shift`+`A` | select all / deselect all (Multi only) | " | " |
| `Escape` | — | clear the focus ring | end the edit, else clear the ring |
| `F2` | — | — | begin editing |
| `*` / `+` / `-` | tree only: expand the subtree / one level / collapse | — | tree table: the same |
| `←` / `→` | tree only: collapse-or-ascend / expand-then-descend | ±1 tile | ±1 cell (tree column: expand / collapse) |

### The modifier rules, stated once

**`Ctrl` (⌘ on macOS) on a navigation key never changes the selection.** It may
still change the *destination* — in a cell grid `Ctrl+Home` escalates from the
row's start to the table's corner — but it always suppresses the selection
update. GTK4's `GtkListBase` registers every navigation key with a
`(select, modify, extend)` triple whose `modify` variant skips the selection
call; Qt's `extendedSelectionCommand` returns `NoUpdate` for any navigation key
with Control held. Teksilo's arrows already followed this rule; the edge and
page keys now do too, which makes it the general case rather than an exception.

## A row's own controls

A row, cell or tile is not a focus target — the container is — and all five
views take those subtrees **out of the Tab order**. A listbox is one Tab
stop with a cursor moving inside it, and a per-row stop would be worse than
merely non-conforming: only realized rows exist, so the number of Tab stops, and
which rows they belong to, would follow the scroll position.

That leaves anything interactive inside a row with no keyboard route, so a row
publishes one: `BuildContext::set_keyboard_toggle` names what `Space` should do
when the row holds the cursor. `Checkbox` publishes its own, so a checkbox
anywhere in a row, cell or tile is reachable with no wiring at all — whether it
came from `StandardListItem` or from a hand-written cell delegate. Any other
control calls `set_keyboard_toggle` to opt in.

The scope is the smallest navigable unit: `ListView` and `TreeView` look in the
focused **row**, `TableView` and `TreeTableView` in the focused **cell** (a
table can carry more than one checkbox column, so "the row's checkbox" would be
arbitrary), and `GridView` in the focused **tile**.

`Space` therefore means "check this" on a row that has a checkbox — what Windows
does for a checkbox list view, and what a visible checkbox looks like it should
answer to — and `Ctrl`+`Space` keeps meaning "toggle the selection". A row
without a checkbox publishes nothing, so `Space` still moves the selection there.
A tristate row goes Checked ↔ Unchecked and never *sets* Indeterminate, which
belongs to the model's descendant aggregation rather than to a keystroke.

**`Ctrl+↑`/`↓` and `Ctrl+Space` read *physical* Control on every platform**,
including macOS, where the rest of the family reads ⌘. ⌘Space is Spotlight, and
⌘↑/⌘↓ already mean something else in a Finder list, so this Explorer-style
cursor pair has no ⌘ counterpart to move to. The asymmetry is deliberate, and
it is what leaves ⌘↑/⌘↓ free for the macOS aliases below.

**A `Shift` range is recomputed from the anchor, never accumulated.** Reversing
the gesture shrinks it: `Shift+End` then `Shift+Home` leaves one range, not the
whole collection. `Ctrl+Shift` keeps whatever the previous gesture selected, so
a second disjoint range can be built without losing the first. The anchor moves
on a plain click or arrow and on `Ctrl+Space` — in **either** direction, which
is the clause that makes *`Ctrl`+arrow away → `Ctrl+Space` → `Shift`+arrow*
extend from the row just picked. See
[`SelectionModel`](../crates/teksilo-data/src/selection_model.rs).

### Scope: what `Home` is scoped to

A row is a `Home` target **iff `←`/`→` move a cell cursor**. That is the
discriminator every stack uses, and it is not the widget's name:

- Lists and trees have no column cursor, so `Home` is the first item — the ARIA
  listbox and tree patterns, Qt's `QListView`/`QTreeView`, GTK's `GtkListBase`.
- A `TableView` in a **row**-selection mode has no column cursor either, so its
  `Home` is the first *row*. This is what Explorer's details view does. Before
  this, `Home` moved the column in every mode, which made `Shift+Home` an
  effective no-op in the default `MultiRow`.
- A `TableView` in a **cell** mode does have one, so `Home` is the row's start
  and `Ctrl+Home` the corner — the ARIA grid pattern and Qt's `QTableView`.
  A `TreeTableView` in a cell mode keeps the *column* on `Ctrl+Home`: the ARIA
  treegrid pattern says "the cell in the first row in the same column as the
  cell that had focus", where the grid pattern says the corner. The two
  disagree deliberately, and both widgets share one keyboard module, so this
  is the one place it branches on which pattern the table is.
- A `GridView`'s rows are a **reflow artifact**: they change with the window
  width, so the same keypress would land somewhere different after a resize.
  `Home` is therefore the absolute first tile, which is what `GtkGridView` and
  Qt's `QListView` in icon mode both do. See the deviations below.

### Paging

Every view pages by **geometry**, not by `index ± page_size`: the row-offset
table (`common::row_metrics`) for the four row views, the layout strategy's
real tile rectangles for `GridView`. With variable or auto-measured heights the
two answers differ, and the estimate drifts as measurements converge. Paging
always moves at least one row, even when a single row is taller than the
viewport.

## Two deliberate deviations

Both are recorded here so there is a link to hand whoever files them.

### macOS: `Home`/`End`/`PageUp`/`PageDown` move the selection

AppKit does not. `NSTableView` implements `scrollToBeginningOfDocument:` and
**not** `moveToBeginningOfDocument:`, so on a Mac these four keys scroll the
view and leave the selection where it is, and `Shift+Home`/`Shift+End` are dead
chords with nothing in the responder chain to service them. The native
consequence is that *no key selects the first row*.

Teksilo does not reproduce that, for the same reason no other cross-platform
toolkit does: GTK4, Qt's item views, wxWidgets, Slint and VS Code all bind these
four identically on all three platforms. GTK4's entire list key-binding block
contains exactly one `#ifdef __APPLE__`, and it wraps *select-all*. A framework
that shipped AppKit's reading would ship a list a keyboard cannot reach the top
of.

What Teksilo *does* add on macOS is a short list of chords that are dead
otherwise and mean something specific there:

| Chord | Meaning | Precedent |
|---|---|---|
| `⌘↓` | activate the focused row | Finder "Command–Down Arrow: Open the selected item"; VS Code's `list.select` macOS secondary |
| `⌘↑` | collapse, else ascend to the parent | Finder's enclosing folder; VS Code's `list.collapse` macOS secondary |
| `⌥→` / `⌥←` | expand / collapse a whole subtree | AppKit's `expandItem:expandChildren:`, documented as the Option-click twin |

`⌘↑`/`⌘↓` are deliberately **not** first/last. That is the *text* idiom; in a
Mac list those chords already mean parent and open, so binding them to the ends
of the collection would collide with the platform rather than conform to it.
`⌥→` is macOS-only for the mirror-image reason: on Windows `Alt+Right` is
history-forward.

### `GridView`: `Home` ignores the ARIA grid rule

The ARIA grid pattern says `Home` moves "to the first cell in the row that
contains focus". That rule is written for a data grid, where a row is a
semantic unit. In a wrapped tile grid it is not: the row exists only because
the tiles wrapped at the current width. `GtkGridView` and `QListView` in icon
mode both resolve `Home` absolutely, and so does Teksilo. Row and column
indices are still published accurately, so assistive technology can still
report a tile's position.

## Chords Teksilo deliberately does not bind

| Chord | Why not |
|---|---|
| `Backspace` = go to parent | GTK and the Win32 tree view both do it, but the Explorer *shell* takes it for history-back and it clears the type-ahead buffer. `←` already covers it. |
| `Alt+Right` = recursive expand | History-forward on Windows. macOS gets `⌥→`, where no such conflict exists. |
| `Escape` = clear the selection | No platform documents it. `Escape` is cancel-edit → cancel type-ahead → close popup. |
| `Ctrl+Alt`+*arrows/Home/End/Page* | Owned by NVDA and JAWS for table reading, and `Ctrl+Option+…` by VoiceOver. |
| Numpad-only `*` / `+` / `-` | Microsoft's own interaction guidelines say not to bind keypad-only keys; both key locations produce the same character anyway. Note the corollary: `Shift` is *not* rejected on these three, because `*` and `+` are shifted keys on every Latin layout and the modifier state still reports it. |

## Type-ahead

Opt-in per view via `.type_ahead_label(…)`. A printable character with no
`Ctrl`/`Alt`/`Super` (`Shift` **is** allowed) moves the cursor to the next row
whose label starts with the accumulated term; repeating the same character
cycles among the matches rather than growing the prefix, which is what the ARIA
patterns and every desktop implementation do. Case folding is full Unicode, so
an accented label is reachable.

The 500 ms reset (`.type_ahead_timeout`) sits between Qt's 400 ms and Dolphin's
1000 ms. Windows derives its own from the double-click time — 4× natively,
2× in WPF — so there is no single number to match.

In a **table**, a printable key opens the cell editor instead where the column
declares `EditTriggers::ANY_KEY`. A grid cannot have both type-to-search and
type-to-edit on bare letters, and the editor wins; the WinForms default
(`EditOnKeystrokeOrF2`) makes the same choice.

## Accessibility

- A selectable `TableView` announces `Role::Grid`, not `Role::Table`.
  AccessKit's consumer will not treat `Table` as a selection container, so a
  multi-select table announced that way exposed no selection at all to UIA.
  `Role::Table` is kept for `SelectionMode::None`, where the static-structure
  role is the right one.
- In a cell-selection mode the cells announce `Role::GridCell`, which is the
  only cell role AccessKit gives the UIA `SelectionItem` pattern.
- Focus and selection are published as two independent facts. `Ctrl`+arrow
  exists precisely to make them disagree, and binding active-descendant to the
  selection would hide disjoint selection from assistive technology.
- All five views **advertise and** answer `Action::ScrollIntoView`, the one
  scroll action every AccessKit adapter consumes. Advertising is the load-bearing
  half: each adapter gates its scroll pattern on the node *supporting* the
  action, so a handler installed without one is invisible to a real screen
  reader. The tree views answer `Action::Expand` and `Action::Collapse`, on
  branch rows only — a leaf that advertised them would be the same
  advertised-and-inert bug one level down.

**Known upstream limitations.** A selected *row* does not report `IsSelected`
on Windows: `Role::Row` is absent from `accesskit_windows`' selection-item list,
which carries its own `// TODO: tables (#29)`. And `Role::TreeGrid` maps to
`NSAccessibilityTableRole` rather than `OutlineRole`, so a `TreeTableView`'s
hierarchy reads flat under VoiceOver while a `TreeView`'s does not.

## Testing both platforms from one host

`ListNavConvention::CURRENT` is a `cfg!` constant, so a Linux test run would
otherwise only ever see half of the rule — and the half it could not see is the
one the rule exists for. Every convention-dependent function therefore has a
`_for` twin taking the convention explicitly, the same split
[`common/text_nav.rs`](../crates/teksilo-widgets/src/common/text_nav.rs) and
[`Modifiers::with_command_convention`](../crates/teksilo-core/src/event.rs) already
use. What the twins cannot test is *delivery*: that `Fn+←` arrives as
`NamedKey::Home` on a Mac is only observable on one.
