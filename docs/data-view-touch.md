<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Data views under a finger

The five data views — [`ListView`](../crates/teksilo-widgets/src/list_view.rs),
[`TreeView`](../crates/teksilo-widgets/src/tree_view.rs),
[`TableView`](../crates/teksilo-widgets/src/table_view.rs),
[`TreeTableView`](../crates/teksilo-widgets/src/tree_table_view.rs) and
[`GridView`](../crates/teksilo-widgets/src/grid_view.rs) — answer one touch
contract, the way [data-view-keyboard.md](data-view-keyboard.md) is one keyboard
contract. This page is that contract, and the two rulings behind it.

Everything here is about a **direct** pointer — a finger or a pen. A mouse
behaves exactly as it always has, and every rule below says how that is
arranged, because "the mouse must not move" is the constraint the design was
built under rather than a hope.

## 1. When a press commits

A press from a mouse is already a click: nothing else can want it, so a row
selects the moment it goes down. A press from a finger is not. The same contact
is the opening sample of a scroll, and a scrolling finger must leave the
selection exactly as it found it.

That cannot be arranged by checking something at the release. A selection
written in the `PointerDown` handler is already written by the time any release
is dispatched, whatever the release then decides. So the **write itself moves**:
for a direct pointer, [`data_views::deferred_select`] records what the press
decided — plain, accelerator or shift — and applies it on the release, where
`release_completes_the_press` can still refuse it.

The table below says "row"; a `GridView` **tile** is the same commit site under
the same rule, and every line of it holds for a tile too.

| | mouse / pen-as-cursor | finger, pen |
| --- | --- | --- |
| plain press on an unselected row | selects now | records; selects on release |
| accelerator press | toggles now | records; toggles on release |
| shift press | extends now | records; extends on release |
| plain press on an already-selected row | records; collapses on release | records; collapses on release |
| the nav cursor | moves with the selection | moves with the selection |

The last row of that table is the one deferral a mouse always had: pressing an
already-selected row keeps the whole multi-selection alive so a drag can carry
all of it, and collapses to the pressed row only on a release without a drag —
the Explorer / Finder convention. `TableView`'s **cell** selection is a second
commit site with its own coordinate pair and its own model; it carries the same
rule separately.

A release is refused in the two ways a row loses the press without seeing a
cancel: a peer claimed the contact (the scrollable above it won its `PanClaim`,
so the press is now a scroll), or the pointer left the press's tap boundary.
Refusal *clears* the recorded decision rather than postponing it, so it can
never fire on some later release the row does own.

**The claim this earns, stated exactly.** A pan commits nothing a **click**
would have committed — whichever half of the click the widget used to act on.
It is not "a pan changes no selection": an accessibility action, a keyboard
step, or the application's own code may change the selection during a pan, and
none of those is the pan's doing.

## 2. Reorder, marquee, and the hold

A drag and a scroll want the same gesture from the same contact.
[`DragActivation`](../crates/teksilo-tokens/src/input.rs) settles it: a precise
pointer latches immediately, a direct one waits for a long press so the
scrollable underneath gets first refusal.

The policy binds only a drag hung on a node that **strictly encloses** whatever
captured the press — that is the one place the tree reads it. A drag on the
capturing node itself is invisible to it, and worse: the router dispatches a
move to the captor *before* it advances the arbitration, so such a drag latches
at `drag_slop` (18 dp) and decides the sequence before any claim can be
evaluated at `pan_slop` (36 dp). A view arranged that way neither scrolls nor
drags.

So a no-op tap (`data_views::press_absorber`) guarantees a gesture arena to
whatever a `data_views::DragSurface` will enclose — a row of the four row views
when that row is a drag source (`reorderable` or `exportable`; where there is no
surface there is nothing to arm), and `GridView`'s body pane, which carries the
marquee — and the reorder or the marquee hangs on that surface one node further
out. **Every** grid tile takes the tap as well, drag source or not, for a reason
that has nothing to do with dragging: the pane above it holds one, so a tile
without an arena of its own loses the press to the pane, and a release
dispatched to the pane and bubbled target→root never reaches a tile below it.
The result:

- **a mouse** resolves `Auto` to `Immediate` and latches at 5 dp, exactly as
  before;
- **a finger** resolves it to `AfterLongPress`: a pan wins, and a contact held
  still past the profile's `long_press` reorders (or marquees) instead.

**One known limit, and it is a defect rather than a design.** A deferred drag is
revoked if the first sample after the hold lands outside the row that was
pressed — a coarse pointer's press boundary is the pressed node's own bounds, and
ending the press takes the drag member with it. `drag_slop` is 18 dp and a
default `TreeView` row is 28, so on short rows a real finger's first reported
sample is as likely as not to be outside, and the reorder silently becomes a
scroll. The measurements, and the question for whoever owns the sequence, are on
`a_finger_reorder_survives_its_first_sample_leaving_the_row` in
[`data_view_drag.rs`](../crates/teksilo-widgets/tests/data_view_drag.rs), which is
`#[ignore]`d against it. The grid's marquee is unaffected: its press is captured
by the body pane, whose bounds are the whole viewport.

### A second known limit: an ancestor that captures the press

A plain row — no reorder, no export — carries no gesture arena of its own, and
does not need one while nothing above it competes. An **application** can put one
there by wrapping the view in anything tappable, and then the wrapper captures
the press, the release is dispatched to the wrapper and bubbled wrapper→root, and
the row never sees the release its press deferred. Measured: a plain `ListView`
inside a `ZStack` carrying an `on_tap`, selection preset to `[0]`, a finger tap on
row 5 → the selection stays `[0]`. A mouse still selects, because a mouse commits
on the press and the press bubble does reach the row.

This is the same mechanism that made `GridView` tiles select nothing under a
finger, where the competing captor was the grid's *own* body pane and so was
present by default. The obvious fix — the absorber applied unconditionally at the
four row sites, as `GridView` now applies it to tiles — was tried and measured: it
fixes this, and it breaks `TableView`'s **cell** selection under a finger, because
the row's new arena takes the press the cell needs in order to commit on its
release. So the fix is the four sites *plus* a cell-level absorber *plus* a ruling
on which node owns a press when an application wraps a data view in a tappable
container. `a_finger_tap_on_a_list_row_under_a_tappable_ancestor_still_selects_it`
in [`data_view_selection.rs`](../crates/teksilo-widgets/tests/data_view_selection.rs)
is `#[ignore]`d against it and carries the numbers.

### The long-press collision ruling

Where a row has both a reorder and a context menu, **the reorder wins the
hold.** A held contact on a draggable row is picking it up; that is the gesture
users arrive with from every touch list they have used. The row's context menu
therefore needs another route, and three are already there: an overflow
affordance the application places in the row, the secondary button, and
Shift+F10 / the AccessKit `ShowContextMenu` action — the last of which is not a
pointer gesture at all and so cannot collide with anything.

**Both halves of that ruling are now delivered**, by the tree-owned touch route
([`touch_route`](../crates/teksilo-core/src/widget_tree/touch_route.rs)):

- a reorderable row's hold no longer fires the row's own `on_long_press` as well.
  The framework's implicit predicate is the **deferral itself** — a live sequence
  member whose activation was put off to the long-press deadline — but it is keyed
  on that member's own node, and a row's drag lives one level out on its
  `DragSurface` wrapper, so what actually answers here is the explicit
  `LongPressRole::DragHandle` that `data_views::row_grab_surface` puts on the
  wrapper. Either way it needs no cooperation from the row, whose handler belongs
  to the application's delegate and which no data view could gate.
- a row with **no** reorder opens its context menu on a hold, through the same
  `show_context_menu_for` a secondary press reaches.

**A mouse keeps its hold, on a reorderable row as much as on a plain one** —
because on a row both claims are *inferred*. It enrols no pan competitor, so
`DragActivation::Auto` is never resolved to `AfterLongPress` on its sequence and
nothing is deferred by that route; and `long_press_is_a_grab` walks the
`DragHandle` ancestors only for a direct pointer, because a mouse latches its
drag on travel and so spends no hold on one. That second half is a gate, not a
construction: without it, `.reorderable(true)` / `.exportable(..)` silently
deleted the row delegate's `on_long_press` under the mouse. The pair
`a_reorderable_rows_hold_does_not_also_fire_its_own_long_press` (finger, silent)
and `a_mouse_hold_on_a_reorderable_row_still_fires_its_own_long_press` (mouse,
fires) in [`data_view_drag.rs`](../crates/teksilo-widgets/tests/data_view_drag.rs)
pin both, and must disagree.

What a mouse does *not* keep is a hold a node asked for by name. An application
that puts `DragActivation::AfterLongPress` on a node itself — rather than
leaving the framework to infer it — is declaring that the hold is that node's
drag-start route, and `PointerSequence::resolve_activation` passes a declared
activation straight through instead of synthesising one. So that node's grab is
deferred to the hold for every pointer kind, and its own `on_long_press` is
suppressed under a mouse as well. No data view declares one, which is why a row
is not affected;
`an_explicitly_deferred_grab_takes_the_hold_from_every_pointer_kind` in
[`touch_route.rs`](../crates/teksilo-core/src/widget_tree/touch_route.rs) pins
the case so the two rules are not confused for one.

## 3. The column-header strip is a pan surface

`TableView` and `TreeTableView` scroll from their header strip — the band across
the top of the view (32 dp by default), and exactly where a thumb lands. The header cell therefore
answers `Ignored` to a plain press rather than claiming it: a `Handled` in the
root-first preview pass is read as a preview claim and decides the sequence,
which is what used to leave the strip dead to a finger in both axes.

Two consequences worth knowing:

- the sort cycle fires on a release that still completes its press, and that one
  question answers both halves. `release_completes_the_press` is refused when a
  peer claim took the press — which is how a header press that a scrollable's
  `PanClaim` won stops sorting the table it just scrolled — *and* when the
  pointer left the press's `TapBoundary`, which is the movement check, sized for
  the pointer holding it: a `tap_slop` radius for a mouse, the pressed cell's own
  bounds for a finger. The cell measures no distance of its own; that would be a
  second hardcoded radius for the framework's own question;
- a **column reorder** waits for a hold, like the row reorder does, but by a
  different mechanism. It escalates from a raw `PointerMove` at 5 dp, and no pan
  slop is below that; the cell keeps receiving moves for as long as the contact
  is over it, so a horizontal swipe along the strip used to pick a column up
  instead of scrolling the table sideways (measured: `max_scroll_x` 512,
  `scroll_x` 0, the columns swapped). A raw `start_drag` is not a member of the
  pointer sequence, so the tree resolves no `DragActivation` for it — the cell
  reads a `long_press` recognizer instead, which is the hold source that path
  has. A swipe therefore pans; a held contact reorders. One knock-on: the hold on
  a header cell is now the reorder's, so anything that later wants a header
  context menu has to share it.

## 4. Drop bands on a tree row

A drop on a hierarchical row lands **before** it, **into** it, or **after** it,
by where in the row's height the pointer is. Those were plain thirds — 9.33 dp
each on a default 28 dp tree row, which is fine for a cursor and too fine for a
fingertip.

[`common/drop_bands.rs`](../crates/teksilo-widgets/src/common/drop_bands.rs)
owns the rule now, and all four sites read it — both views' `on_drag_hover`
(which decides the affordance the user sees) and both `on_drop` (which decides
what happens), so the insertion line cannot promise a position the drop does not
take. A coarse pointer widens the two **edge** bands toward a floor and `Into`
takes what is left, because `Before`/`After` are the universal reorder while
`Into` is the tree-only reparent and has two other routes: the spring-load (hover
a branch, it expands, and its children offer a `Before` at the position wanted)
and the keyboard.

**The floor is not always reachable, and the module says so rather than
pretending.** Three bands at the coarse floor need three times it of row; a cap
keeps `Into` alive instead of letting the floor eat it, so below the row height
at which the cap and the floor cross — the floor divided by the cap's fraction —
the edge bands are the widest they can be while leaving `Into` reachable at all,
and that is narrower than the floor. Every band has positive
extent at every *positive* row height, which is the invariant that matters: a band of zero
would be a drop position no pointer could express. A precise pointer keeps the
plain thirds at every height.

The bands do not move with `TargetDensity`. Density describes how big the UI's
own targets are; this describes how precisely a *device* can be parked inside a
row whose height the application chose — the same argument
`common::drag_autoscroll` makes for the auto-scroll edge band, which is a
pointer-kind question for the same reason.

## 5. Tests

| What | Where |
| --- | --- |
| a pan scrolls each view, and commits nothing a release would have | [`scrollables_touch.rs`](../crates/teksilo-widgets/tests/scrollables_touch.rs) |
| what a press commits, and when | [`data_view_selection.rs`](../crates/teksilo-widgets/tests/data_view_selection.rs) |
| reorder, marquee, the hold, the header strip, and the drop bands through a real view | [`data_view_drag.rs`](../crates/teksilo-widgets/tests/data_view_drag.rs) |
| the band arithmetic itself | inline in `common/drop_bands.rs` |
| the *affordance* reading the same bands as the drop | `tree_view/tests.rs` |

[`data_views::deferred_select`]: ../crates/teksilo-widgets/src/data_views.rs
