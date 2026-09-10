<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Single-pointer alternatives to dragging

WCAG 2.2 **SC 2.5.7 Dragging Movements** (AA) says that any function operated by
a dragging movement must also be operable with a single pointer without
dragging. It is a requirement about *paths*, not about pointers: a mouse user
with a tremor, a finger on a small screen, a head-pointer, a switch device and an
assistive client all fail the same way on a drag.

Teksilo's answer is that a widget which reorders, resizes or relocates something
by drag also offers the same operation as a command. This page is the enumeration
of those commands, the vocabulary they share, and the rule for adding one.

The exhaustive list of dragging operations in the workspace, with the state of
each, is [the drag census](../drag-operation-census.md). This page is what
discharges it.

## Four obligations, not one

A command that exists but cannot be reached, or can be reached but does something
different from the drag, is not an alternative. So every operation below carries
four things, and they are not interchangeable:

1. **A real menu row** — a row in a menu the user can open, not an API an
   application has to wire.
2. **A keyboard route** — reachable with no pointer at all.
3. **An AccessKit custom action** — for a client that has neither. A node's
   custom actions are only reachable when the node also advertises
   `Action::CustomAction`; the framework adds it wherever it advertises any.
4. **One utterance on commit** — through `EventContext::announce`. Silence after
   a keyboard command is indistinguishable from a refusal.

The load-bearing one is unwritten above and is the reason for the shape of the
implementation: **the alternative must make the same model change the drag
makes**. Each operation's tests assert that by driving the drag and the
alternative over the same model and comparing.

## One mechanism, four surfaces

Reordering appears in nine of the census's drags, and the four obligations are
the same four in all of them, so they are written once in
[`common::ordered_move`](../../crates/teksilo-widgets/src/common/ordered_move.rs).
Six of the nine use it today; the table at the end of this page says what each of
the rest still needs.

Its central type is `OrderedMove` — four values (`Prev`, `Next`, `First`,
`Last`), not a signed step. "Move to the far end" is one model change and one
utterance, where repeating a step would be many of each.

The important property is that a consumer builds **one closure** and the menu
row, the chord and the custom action all call it. Three implementations of one
command can disagree; one cannot. `RowCommands::install` is the whole per-row
wiring, and the keyboard handler calls the same closure it was given.

The commit itself travels as the `(target, position)` pair a *pointer drop*
carries, never as a destination index. The five data views commit a reorder by
handing that pair to the bound source's `accept_drop` / `reorder_within`, and the
source — not the view — owns what it means. Routing the alternative around that
would be a second implementation of the same operation, which is exactly what an
alternative must not be.

## The keyboard vocabulary

`Alt` plus a direction, with one platform exception noted below. It is what
outliners and tab strips already use for this, and it is what the data views that
already had a keyboard reorder used, so it is not new vocabulary.

| Chord | Meaning |
| --- | --- |
| `Alt` + the collection's own two arrows | one place earlier / later |
| `Alt+Home` / `Alt+End` | to the first / last position |
| accelerator + `]` / `[` in a tree | indent / outdent — the reparenting drop |
| `Alt+Right` / `Alt+Left` in a tree, off macOS only | the same two |
| `Alt+Up` / `Alt+Down` in a grid | one whole row earlier / later |

The tree's reparent has two spellings because macOS spends `⌥→` / `⌥←` on
expanding and collapsing a whole subtree, which AppKit's own outline view claims;
binding the arrows there would take a working chord away to add one that already
has another spelling. The bracket pair works on every platform, with
`Modifiers::command()` making it ⌘ on macOS and Ctrl elsewhere.

Two directions read off the screen rather than off the order, and so mirror under
RTL: the **horizontal** arrows of a horizontally-ordered collection, and the
arrow spelling of a tree's reparent. `Home`, `End`, `]` and `[` never mirror —
they name positions in the order, which does not flip.

A move that would change nothing is not offered at all: no menu row, no custom
action, nothing announced. That is what keeps a row already at the top from
advertising a move to the top.

## Where the menu rows live

The framework installs its own context menu on the thing being moved — the row,
the tile, the tab header. Two consequences worth knowing:

- An application's own per-row, per-tile or per-tab menu **wins**: an application
  that writes one owns that surface's menu. The custom actions and the chord are
  unaffected, so the command stays reachable by two other routes. How the
  precedence is arranged differs by widget and is stated at each site — a factory
  nearer the click wins the context-menu walk, and where the framework and the
  application would install on the *same* node the framework installs first so
  the application's replaces it.
- The menu appears only where the drag does. A view that was never made
  reorderable gains nothing: the obligation is to make an existing drag
  reachable, not to add one.

## What is announced

One utterance per completed command, and only on a command that changed
something. A refused move says nothing — there is no event to report, and
"moved" when nothing moved is worse than silence.

A reorder names the new position within the set the move was made in: for a tree
that is the row's own siblings, not the flattening, because that is the set the
move was about. A reparent names the new **level** instead, because that is what
changed. A 2-D value names both of its axes, because both of them are the value.

Where the widget already speaks for itself, the framework does not speak over it.
A `GridView` publishes its selection as a live value, and a move that carries the
selection forward leaves that value unchanged, so the move is the only thing
said.

## Operations with a route

Each row cites the census row it discharges. Every one of them has all four
obligations and a test per obligation in
[`tests/non_drag_alternatives.rs`](../../crates/teksilo-widgets/tests/non_drag_alternatives.rs).

| Census | Operation | Commands |
| ---: | --- | --- |
| 9 | `ListView` row reorder | the four moves |
| 10 | `TreeView` row reorder **and reparent** | the four moves among siblings + indent / outdent |
| 11 | `TableView` row reorder | the four moves |
| 12 | `TreeTableView` row reorder **and reparent** | the four moves among siblings + indent / outdent |
| 13 | `GridView` tile reorder | the four moves + a whole-row step |
| 16 | tab reorder within a bar | the four moves |
| 4 | the saturation × brightness field | a step per axis, plus the numeric entry beside it |

The colour field is the one that is not a reorder, and it is the one where the
menu obligation is discharged by a control rather than a menu row: a colour field
has no menu to put rows in, and its numeric entry is on by default so an
application does not have to ask for it. In the compact picker layout there is no
room for that entry, which is why the field's own arrow keys are the route that
exists in every layout.

## Adding one

For a new widget that reorders something by drag:

1. Build one closure that performs the move through whatever path the drop takes,
   and announces. For a data view that is `RowMover::commit`, which does both.
2. Hand it to `RowCommands::install` on the node being moved. That is the menu row
   and the custom action.
3. Decode the chord with `OrderedMove::from_key` (and `TreeMove::from_key`, for a
   widget that also reparents) in the widget's `on_key`, and call the same
   closure.
4. Write the five tests: it exists, it is keyboard-reachable, it is advertised as
   a custom action, **it makes the same model change the drag makes**, and it
   announces once.

Step 4's fourth item is the one that cannot be skipped, and the way to write it
is to drive the real drag in the same test and compare the model. A test that
asserts a menu row is present passes when the row does nothing.

## What is still drag-only

These are open, with what each needs. The census carries the full reasoning.

| Census | Operation | What it needs |
| ---: | --- | --- |
| 7, 8 | `TableView` / `TreeTableView` column resize and reorder | a focusable header row with a roving tab index — the prerequisite both share, since the header cell takes no focus today. `Ctrl+Shift` plus the horizontal arrows, which the census proposed, is not available: `table_view/keyboard.rs`'s arrow arm already reads it as an additive selection extension. |
| 14, 17, 19, 29 | cross-widget row export, cross-bar tab transfer, the five-zone dock drops, `DropTarget` drops | a framework-level keyboard pick-up / put-down mode (census row 30). All four are the same shape — a payload leaving one surface for another — and all four are retired by that one mechanism. |
| 21 | activity-rail reorder | the rail's own `on_key` plus two rows in `docking/context_menu.rs`, using the visible-index mapping its drop handler already computes. |
| 22 | `ToolBox` section header drag | a paired `on_header_move(index, direction)` hook the widget calls, so every consumer of `on_header_drag` inherits an alternative rather than writing one. |
| 23 | embedded-image resize in rich text | image commands in `rich_text/context_menu.rs`, which has none, plus a numeric size entry. |
| 26 | scene marquee selection | a focus ring roving over lightweight items, which have no `WidgetId` to focus; `Ctrl+A` alone does not reach every selection a marquee can make. |
| 31, 32 | window move and resize under custom chrome | `Move` and `Size` rows in `title_bar/window_menu.rs` driving a keyboard nudge mode — the Win32 system-menu convention. Only where Teksilo's **fallback** menu is in use: where the platform owns a window menu, a long press or a secondary click asks the OS for it and its own Move entry is the route. |
| 37 | terminal range selection | a scrollback selection cursor; the terminal has no caret for `Shift`+arrow to extend from. Word, line and select-all already need no drag. |

## See also

- [The drag census](../drag-operation-census.md) — every dragging operation in the
  workspace, and the state of each.
- [Accessibility overrides](../accessibility-overrides.md) — the `.access_*`
  surface, including `access_custom_action`.
- [Density and targets](../density-and-targets.md) — SC 2.5.8, the other pointer
  criterion.
