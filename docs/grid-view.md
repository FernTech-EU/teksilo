<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# GridView: Virtualized 2D Tile Grid

`GridView<T>` is the photo-gallery / icon-view / file-manager-grid /
collection-view widget, the 2D sibling of [`ListView`](widgets-overview.md)
and `TableView`. It is bound to a `ListModel<T>` / `ListDataSource`, realizes
only the tiles currently visible (plus a buffer), reflows on resize, and is
supports keyboard navigation and AccessKit.

Source: [crates/teksilo-widgets/src/grid_view.rs](../crates/teksilo-widgets/src/grid_view.rs)
(+ `grid_view/` submodules). Demo: `cargo run -p grid-view`.

```rust
use teksilo::widgets::{GridView, GridSizing, grouping_sections};

GridView::new(model, |tc| {
    Box::new(card_for(tc.item, tc.is_selected))
})
.sizing(GridSizing::Adaptive { min_width: 140.0, max_width: Some(220.0), height: 110.0 })
.spacing(10.0)
.selection(selection_model)         // Multi → marquee + Ctrl/Shift
.reorderable(true)
.sections(grouping_sections(&model, |p| p.album))
.pinned_section_headers(true)
.a11y_label("Photo library")
```

## Layout strategies

A pluggable `GridLayoutStrategy` drives virtualization; three ship:

| Strategy | Selected by | Heights | Notes |
| --- | --- | --- | --- |
| **Uniform** (default) | `.sizing(...)` / `.tile_size` / `.column_count` | fixed | Exact O(1) positions. The common photo/icon grid. |
| **Variable row** | `.variable_row_heights(estimated)` | each row = tallest tile | SwiftUI `LazyVGrid`. Auto-measure + scroll-anchoring, or exact via `.item_height(i)`. |
| **Waterfall** | `.waterfall(estimated)` | per-item | Pinterest column-balanced flow. No scroll-anchoring (items reflow across columns). O(n) per layout on height change, fine for hundreds–low-thousands. |

**Tile sizing** ([`GridSizing`]):

- `Fixed { width, height }`, exact tile size; column count derived (tiles not stretched).
- `FixedColumnCount { count, height }`, exactly `count` stretched columns.
- `Adaptive { min_width, max_width, height }`, fit as many ≥ `min_width` columns as possible, stretch up to `max_width` (CSS `repeat(auto-fill, minmax(...))` / Flutter `maxCrossAxisExtent`).

Sugar: `.tile_size(w, h)`, `.column_count(n, h)`. Spacing: `.column_spacing`,
`.row_spacing`, `.spacing` (both), `.content_inset(EdgeInsets)`.

**Column bounds and alignment.** `.min_columns(n)` and `.max_columns(n)` bound
the count `Fixed` and `Adaptive` derive from the width; `FixedColumnCount`
ignores both (its count is the answer). When `min_columns` asks for more
columns than a `Fixed` width affords, the tiles **narrow** to share the width
(never horizontal overflow); the tile **height** stays what the sizing says, so
a tile that must hold an aspect ratio sizes its own content from the width it
is handed. If the minimum exceeds the maximum, the maximum wins. Without a
`min_columns`, a `Fixed` tile keeps its width even in a viewport narrower than
one tile.

A block of columns narrower than the content (a `Fixed` width, an `Adaptive`
`max_width`, a `max_columns` cap) is placed by `.column_alignment(HAlignment)`:
`Leading` (the default: tiles against the leading inset, leftover trailing),
`Center`, or `Trailing`. Leading and trailing swap in a right-to-left layout;
the order of the columns within the block is the same in both directions. Only
the columns move: a section header and a [detail band](#detail-band) span the
content width, and the marquee, drag-reorder insertion bar, focus ring and
accessibility bounds all follow the offset.

An album grid of fixed 182 dp cards at a fixed pitch, two to ten columns, the
leftover split on both sides rather than stretched into the cards:

```ignore
GridView::new(albums, |tc| album_card(tc))
    .sizing(GridSizing::Fixed { width: 182.0, height: 230.0 })
    .min_columns(2)
    .max_columns(10)
    .column_alignment(HAlignment::Center)
```

**Reactive sizing.** `.sizing(...)` accepts `impl Into<Prop<GridSizing>>`, so it
takes a plain `GridSizing` **or** a `Signal<GridSizing>`. A bound signal is
observed at `BindingLevel::Rebuild`: changing it rebuilds the cached layout
strategy and reflows the grid, the internal `scroll_y` / `focused_index` /
selection are field signals on the same widget instance, so they survive the
rebuild (no scroll jump). This is the "card-size slider" path, drive a
`Signal<GridSizing>` from a `Slider` and the tiles resize live. (`.tile_size` /
`.column_count` set a static size and clear any bound signal.)

### Variable heights under virtualization

Off-screen tiles aren't built, so their heights are unknown. Two paths:

- **Auto-measure** (default for `variable_row_heights` / `waterfall`): the body
  pane measures each realized tile (`ctx.child_size`, height-for-width) and feeds
  it back. Unmeasured rows use the estimate. When a corrected estimate shifts
  content at/above the viewport top, `VariableRowGrid` adjusts `scroll_y` to keep
  it visually stationary (one-frame latency, no jump). Backed by a prefix-sum
  offset table with O(log n) row↔y lookups (`PrefixSumOffsets`, shared with the
  1-D row widgets, `ListView` / `TreeView` / `TableView` / `TreeTableView`, from
  `common/row_offsets.rs`). After each measure pass a *realization re-check*
  compares the corrected visible range against the realized tile range and
  requests a rebuild when tiles measured shorter than the estimate would
  otherwise leave a gap at the viewport bottom, convergence is guaranteed by
  the sub-pixel measurement epsilon. When a measure pass changes the content
  total, the pane pokes the container (a `Relayout`-bound signal) so
  `max_scroll_y` and the thumb ratio, computed parent-first, before the
  measurements, are re-derived next frame; without the poke, content past the
  estimated total would stay unreachable until the next scroll.
- **Exact** (`.item_height(index)`): row heights are seeded exactly as
  `max(item_height(i))` over the row, exact scrollbar, zero jitter, no
  measurement.

Because `PrefixSumOffsets` is shared with the 1-D row widgets, a
zero-height row (`.item_height` has no floor above `0.0`) hit-tests the
same way here as it does for `TableView`/`TreeTableView`'s drop targeting,
`row_at`'s raw result is the hit-tested tile index, so see
[table-view.md "Which row a `y` coordinate resolves to"](table-view.md) for
the degenerate-height tie-break.

## Selection

Pass a flat `SelectionModel` (`None` / `Single` / `Multi`). Mouse: click =
select, Ctrl+click = toggle, Shift+click = reading-order range (Finder /
Explorer). Ctrl+A = select-all, a no-op in `Single`/`None` mode, matching
`ListView` and `TableView`; the grid's handler gates on `Multi` (and leaves the
chord unclaimed otherwise), and `SelectionModel::select_all` enforces the same
rule for any other caller. `Multi` mode adds **rubber-band marquee**, a
drag on the empty background sweeps a rectangle and selects every intersecting
tile (Ctrl/Shift at drag-start = additive). The hit-test is geometric, so it
selects tiles outside the realized window. `.on_selection_changed(|set|)` fires
on every change (interactive or programmatic), so it carries no input modifiers;
`.on_tile_activate`, which receives an `EventContext`, is the callback for
modifiers at the moment of input. `.marquee_selection(false)`
disables marquee.

### Keyed selection

A `SelectionModel` holds positions, so sorting an album grid by artist or year,
or narrowing a filter, leaves it on whatever tiles moved into those positions.
Build the grid with `from_source_keyed` instead and the selection is held by the
source's key:

```rust
let albums = SortFilterListModel::new(model).with_comparator("year", by_year);
let selected = KeyedSelectionModel::<usize>::new(SelectionMode::Multi);

GridView::from_source_keyed(albums.clone(), selected.clone(), |tc| {
    Box::new(album_tile(tc.item, tc.is_selected))
})
```

The contract is `ListView::from_source_keyed`'s:

- the selection stays on its items through a reorder, a filter and a lazy
  source's sliding window (a tile that is not loaded is selected by its key, and
  is shown selected when it loads);
- clicks, Ctrl/⌘-clicks, Shift-clicks, the keyboard and the marquee all write
  keys;
- an item the source no longer shows, removed or filtered out, leaves the
  selection;
- a `ListModel`'s keys are its positions, so over a bare `ListModel` a keyed
  selection stays on positions, through a `replace_all` too, as
  `ListView::from_source_keyed`'s does; a `SortFilterListModel` over it keys
  each item by its place in the model underneath, which a sort or a filter does
  not change;
- `.on_selection_changed(|set|)` still receives positions: those the selected
  keys have when the selection changes. A sort moves them without changing the
  selection, so it does not fire.

In a `Single` selection the keyboard cursor stays on the selected tile through
a sort. Selecting still rebuilds no tile node: each tile watches its own state.

## Keyboard navigation

Focus (the *current* item) is tracked separately from selection and shown by a
painted focus ring. In `Single` mode the two stay one tile: the keys move them
together, and a selection the application sets moves the focus onto it. Matrix
(RTL-aware; horizontal arrows swap):

| Keys | Action |
| --- | --- |
| Arrow ←/→ | ±1 (within row; `.wrap_navigation(true)` to cross rows) |
| Arrow ↑/↓ | ±columns |
| Home / End | first / last item of the collection |
| Ctrl+Home / Ctrl+End | the same, without moving a `Multi` selection (a `Single` one follows the cursor) |
| Ctrl+Arrow | move the focus without touching a `Multi` selection (a `Single` one follows the cursor) |
| Ctrl+Space | toggle the focused tile's selection |
| PageUp / PageDown | ± a viewport of rows + scroll |
| Space | close the open tile's detail band; else check the focused tile if it holds a checkbox, else toggle (`Multi`) / select (`Single`) |
| Enter | with `.detail_row`, open the tile's detail band (in a `Single` selection with no `.on_tile_activate`, also select the tile), or close the open one; then `.on_tile_activate`. Without a band, or on a tile with nothing to disclose: `.on_tile_activate`, else select |
| ↓ on the open tile / ↑ in its band | into the band's first focusable control / back to the tile (see [Detail band](#detail-band)) |
| Esc | clear focus |
| Ctrl+A | select all (`Multi` mode only); Ctrl+Shift+A deselects |
| Alt+Arrow / Alt+Home / Alt+End | reorder the focused tile (when `.reorderable`); Alt+↑/↓ moves it a whole row |
| printable | type-ahead (needs `.type_ahead_label(i)`; `.type_ahead_timeout`) |
| Tab | `.tab_traversal(WithinGrid \| OutOfGrid)` (default `OutOfGrid`); Ctrl+Tab always leaves the grid |

Shift + any navigation extends the selection range. Every navigation scrolls
the new focus into view.

### The cursor from outside

The current tile, the keyboard cursor, is readable and settable through the
grid's handle (see [Scrolling](#scrolling)):

```rust,ignore
let handle = grid.handle();
let current = handle.cursor_signal();            // Signal<Option<usize>>, read-only
let _keep = current.observe(move |tile| status.set(describe(*tile))); // held while needed
handle.set_cursor(Some(12));                     // the ring and the active descendant move
handle.ensure_index_visible(12, ScrollAnchor::Auto);
```

`cursor()` is `None` until the grid has been navigated, clicked or given a
cursor; the keys then start from the first selected tile. `set_cursor` clamps an
index past the end to the last tile (and removes the cursor on an empty grid),
and leaves keyboard focus, the selection and the scroll offset alone; the next
arrow key steps from it. Like a reveal, it may be called before the grid is
mounted.

In a `Multi` selection the cursor and the selection are **separate**. After
`→`, `Ctrl+→`, `Ctrl+↓` in three columns the selection is still tile 0 while
the cursor, the focus ring and the active descendant are on tile 4, and
`selection.select(20)` from outside leaves the cursor on tile 4. Code that acts
on "the tile the user is on" (a status line, context actions, keyboard
extensions of its own) reads the cursor, not the first selected tile. In a
`Single` selection the two are one tile: a selection written from outside
moves the cursor onto it.

An open detail band needs no walk of its own either: `↓` on the open tile
enters the band's first focusable control and `↑` from there returns to the
tile (see [Detail band](#detail-band)).

## Scrolling

`.scroll_y_signal()` / `.max_scroll_y_signal()` / `.viewport_ratio_y_signal()`
expose the reactive scroll state (wire an external `ScrollBar` with
`.show_scrollbar(false)` so it survives rebuilds). `.overscroll_behavior(Chain |
Contain)` controls scroll chaining.

To scroll the grid from outside, take a handle: `.handle()` returns a
`GridViewHandle`, cheap to clone, usable before the grid is mounted and through
all its rebuilds.

```rust,ignore
let handle = grid.handle();
handle.scroll_to_index(30, ScrollAnchor::Start); // force a position
handle.ensure_index_visible(30, ScrollAnchor::Auto); // only if not fully visible
handle.reveal_band(30);                          // the tile and its open band
```

`ScrollAnchor` is `Auto | Start | Center | End`. A handle computes no offset
when it is called: it records the request (a later one replaces one still
pending) and asks for a relayout, and the layout applies it after measuring the
detail band and settling the scroll anchoring. A request made before mount is
applied by the first layout, and one made in the same turn as opening a band
lands where the new layout puts the tile. That is the difference from writing
`scroll_y` yourself: `expanded.set(Some(0)); scroll.set(668.0)` ends at 756,
because the layout keeps the content under the top edge still across the band
opening and takes 668 for an offset of the old layout, while
`expanded.set(Some(0)); handle.scroll_to_index(30, ScrollAnchor::Start)` puts
tile 30 at the top, at 668.

The builder's own `.ensure_index_visible(i, ScrollAnchor)` and
`.scroll_to_index(i, ScrollAnchor)` take the same arguments but compute the
offset when they are called, from the last layout: before the first layout they
do nothing. Both `ensure_index_visible`s leave a fully visible tile alone,
whatever the anchor.

## Touch

Everything a finger does here, where a press commits the selection, why the
rubber-band marquee waits for a hold, and how the reorder gets out of the pan's
way, is the shared five-view contract in
[Data views under a finger](data-view-touch.md).

## Lazy / incremental loading

There is no view-level `on_near_end` hook, incremental loading is a **source
capability**. When bound to a `ListDataSource`, the body pane calls
`request_window(start..end)` for the visible+buffer range each realize pass, and
when the scroll nears the end it consults `can_fetch_more()` → `fetch_more()` to
grow an append-only source. A tile whose item isn't resident yet (`with_item`
returns `None`) and whose `row_state(i)` is `Loading` renders a placeholder at
the estimated tile size instead of being skipped, so selection and focus still
work while the page loads. (`.is_loading(signal)` + `.loading_view(...)` is the
separate, whole-grid "first page is loading" overlay.) See
[data-source.md](data-source.md).

## Drag-and-drop reorder

`.reorderable(true)` enables intra-grid drag (and keyboard Alt+Arrow / Alt+Home / Alt+End). Drags
route through the bound source's DnD capabilities: a tile is draggable only when
the source's `drag(key)` returns `CanDrag`; on hover the geometric
`(target, position)` is validated by `can_accept(query)` (a vertical insertion
bar shows an accepted landing; a rejected one suppresses it); the drop commits
via `accept_drop(commit)`. A `ListModel`-backed source moves the item by
default. `.on_item_drop(|payload, index, ctx| -> bool)` is the escape hatch for
**foreign / external** payloads (cross-view or OS drops) the source's
`can_accept` rejects, it accepts at a flat insertion index, reusing the
framework DnD pipeline.

## Sections & sticky headers

`.sections(provider)` groups the flat model; a header is rendered above each
section's tile band. `grouping_sections(&model, key_fn)` builds a provider by
partitioning consecutive equal-key runs. `.section_header_delegate(|section,
title|)` customizes the header (default: bold title text);
`.section_header_height(h)`. `.pinned_section_headers(true)` keeps the current
section's header pinned to the top while scrolling (one reused slot widget).
Sections compose with the **uniform** tile layout. The flat index space is
unchanged, so selection and keyboard navigation are unaffected.

## Detail band

The "album expansion" of a desktop music library: activating a tile opens a
full-width band directly under the row that holds it (the album's track list),
the rows after it move down, and the band stays under its row when a resize
changes the column count.

```rust
let open = Signal::new(None::<usize>);

GridView::from_source(albums, |tc| Box::new(album_tile(tc.item)))
    .expanded_index(open.clone())               // which tile is disclosed
    .detail_row(|tc| Some(Box::new(track_list(tc.item)) as Box<dyn Widget>))
    .detail_row_when(|tc| !tc.item.tracks.is_empty()) // optional; which tiles have one
    .detail_row_height(|_index| 240.0)          // optional; measured otherwise
    .tile_a11y_label(|i| album_title(i))
```

- `.detail_row(|tc| -> Option<Box<dyn Widget>>)` receives the disclosed tile's
  `TileContext`, the one the tile delegate gets, item included. `None` means
  the tile has nothing to disclose and the band takes no space. It is also
  asked when a tile is activated, to learn whether the tile has anything to
  disclose, and the content it returns then is dropped: keep it cheap and free
  of side effects.
- `.detail_row_when(|tc| -> bool)` says which tiles have a band without
  building one, from the same `TileContext`. A tile it answers `false` for is
  not a disclosure: no expanded state or `Expand` for assistive technology,
  nothing opens on activation or on an `.expanded_index` naming it, and
  `.detail_row` is not asked for it. It is asked as each tile is built (when
  the tile is realized, after a change to the data, when its selectedness
  flips), so answer from the item, cheaply, and agree with `.detail_row`. A
  tile of a lazy source whose item has not arrived counts as having a band
  until it arrives.
- `.expanded_index(Signal<Option<usize>>)` is the disclosed tile. The grid
  writes it when a tile is activated (a click per `.activate_on`, or Enter):
  activating a tile opens its band and scrolls it into view, activating the
  open tile closes it, and `.on_tile_activate` still fires, after the band has
  opened or closed. Activating a tile with nothing to disclose opens nothing
  and leaves a band open elsewhere as it is. Writing the signal from outside
  opens and closes the band, without scrolling. Without it the grid keeps a
  signal of its own.
- `.detail_row_height(|index| -> f32)` sizes the band. Without it the band is
  measured height-for-width at the content width (the viewport less its side
  insets), as the variable-height tiles are, with the same scroll anchoring
  when its height changes above the viewport top.

**Layout.** The band sits one row gap under the open row, and everything after
it moves down by the band and a row gap: in `max_scroll_y`, in the focus ring,
the drop indicator and every scroll-into-view (the keyboard's, `.ensure_index_visible`,
a tile's `ScrollIntoView` action, and focus moving onto a control in the band,
which the grid scrolls into view when the framework asks it to). It is not a
row of the grid: tiles keep their row and column numbers. It works with the
uniform grid, `.variable_row_heights` / `.item_height` (the band goes under the
row's tallest tile) and `.sections` (it opens inside its section and the
following headers move down). A `.waterfall` has no rows, so the band is
ignored there.

A band that opens, closes, moves or changes height above the viewport leaves
the tiles on screen where they are: the scroll offset takes the difference.
When the viewport's top edge is inside the band, the rows under it stay put.
The realized window is the one the viewport would hold with no band, so a band
that changes needs no tile the grid has not realized already.

**Following the tile.** An insert, a removal or a move keeps the band on its
tile, and removing the tile closes it. A reset (a `SortFilterListModel` sort or
filter, `ListModel::replace_all`) keeps it on its tile when the source has item
keys, and closes it when the source has none or the tile is gone. A `ListModel`'s
keys are its positions, which say nothing about where an item went, so a
`ListModel` closes the band on a reset whether it is given to `new`,
`from_source` or `from_source_keyed`.

**Keyboard.** Enter on a tile opens its band. In a `Single` selection with no
`.on_tile_activate` it also selects the tile, as Enter does without a band; a
`Multi` selection is left as it is, so Ctrl+arrows and Enter look at a tile
without collapsing the selection onto it. Enter or Space on the open tile
closes it, and leaves the selection alone. On a tile with
nothing to disclose Enter and Space do what they do without a band. ↓ from the
open tile moves focus to the first focusable control in the band, scrolled into
view; ↑ there, when the control does not use it, returns focus to the grid with
the cursor on the tile. A band with nothing focusable is stepped over like any
other key. Arrows between tiles step over the band, and while focus is in the
band the grid's own keys stand aside, so a key the band's content lets through
moves no tile. A band that closes, or moves to content with nothing focusable,
while focus is inside it hands focus back to the grid with the cursor on its
tile, whatever closed it: a button in the band, a shortcut, a screen reader's
`Collapse`.

**Rubber band.** The marquee selects tiles where they are, so it never selects
the band, and a press on the band starts no marquee.

**Accessibility.** The band is a `Role::Group`, read right after its tile's
row. It is named after its tile's `.tile_a11y_label`, labelled by the tile node
without one, and named "Details of item N" (in the user's language) when the
tile has scrolled out of the realized window and has no node to lend. A tile
publishes `expanded`, offers `Expand` or `Collapse` (which open and close the
band for a screen reader, with no double click needed), and, while open, a
`controls` relation to the band. With `.detail_row_when`, only the tiles it
accepts do. Without it every tile does, one with nothing to disclose included,
because knowing otherwise means building its band's content for every tile the
grid realizes; its `Expand` opens nothing and it stays collapsed. The grid's row and column counts and each tile's position in the set
are unchanged. Opening, closing and moving the band replace no tile node; a
scroll it causes (the reveal of a band a user opened) realizes tiles as any
scroll does.

**Scrolling to it.** `GridViewHandle::reveal_band(i)` scrolls the least that
shows tile `i`'s row and its open band, the row first when they do not fit
together, as the grid does when a user opens a band; with `i`'s band not open it
shows the tile alone. Write `expanded` and call it in the same turn: the band is
measured before the request is applied. See [Scrolling](#scrolling).

**A long band.** `.detail_visible_span_signal()` is the part of the open band on
screen, in band-local coordinates: `max(0, scroll_y − band.y) ..
min(band.height, scroll_y + viewport_height − band.y)`. It is `None` while no
band is open or the open one is entirely off screen, and it is written after the
scroll offset is clamped, only when it changes. A band holding hundreds of rows
realizes only those in the span, a window of rows between two spacers, with no
copy of the grid's placement arithmetic:

```rust,ignore
const ROW: f32 = 24.0;
let grid = GridView::new(albums, album_card);
let span = grid.detail_visible_span_signal().clone();
grid.detail_row(move |tc| {
    let n = tc.item.tracks.len();
    let first = span.map(|s| s.as_ref().map_or(0, |r| (r.start / ROW) as usize));
    let last = span.map(move |s| s.as_ref().map_or(0, |r| ((r.end / ROW).ceil() as usize).min(n)));
    // `window` holds tracks[first..last], refilled when the span moves.
    let window = track_window(&tc.item.tracks, &first, &last);
    Some(Box::new(
        VStack::new()
            .child(FixedSize::new().height(first.map(|f| *f as f32 * ROW)))
            .child(Repeater::new(window, |track| track_row(track)))
            .child(FixedSize::new().height(last.map(move |l| (n - *l) as f32 * ROW))),
    ))
})
```

**Lifetime.** The band widget is a child of the grid, not of the body pane, so
scrolling, resizing and a lazy source loading a window keep its content (a
track list's own scroll and focus). The content is rebuilt when the band moves
to another tile, when its own tile's item arrives (until then it holds a
placeholder), and when the grid's rows change: an insert, a removal, a move,
an update or a reset.

## Other

- `.on_tile_activate(|index, ctx|)`, double-click / Enter (distinct from selection).
- `.tile_context_menu(|index, pos, ctx| -> Option<Box<dyn Widget>>)`, per-tile menu.
- `.empty_view(|| ...)` when the model is empty; `.loading_view(|| ...)` + `.is_loading(signal)` for an overlaid loading state.
- RTL is honored automatically (column 0 draws at the trailing edge; horizontal arrows swap).

## Theming

`GridView` renders tiles through the app delegate, so its only widget-owned
chrome is paint-time decoration. The Tier-3 `GridViewStyle` protocol exposes
that as recipe data, focus ring (`GridFocusRingRecipe`), marquee
(`GridMarqueeRecipe`), drag-insertion bar (`GridInsertionRecipe`), and the
sticky-header surface role. Each method has a default, so a custom style
overrides only what it cares about. Precedence: `.style(...)` per-call →
`theme.style_slots.grid_view` theme-wide → the stock `RecipeGridViewStyle`.
The container itself uses theme roles (`BorderRole::Focused` / `Accent`,
`SurfaceRole::Raised`) by default.

## Accessibility

The container emits `Role::Grid` with the **logical** `row_count` /
`column_count` (not the realized window), `multiselectable` in Multi mode,
`active_descendant` pointing at the focused tile (roving focus), and the
selection count as its value, in the user's language ("3 éléments
sélectionnés"). Each tile is wrapped in `Role::GridCell`
with 1-based `row_index` / `column_index` and its own 1-based
`position_in_set`; the total (`size_of_set`) sits on the `Role::Grid`
container beside the row and column counts, because AccessKit resolves an
item's set size by walking *up* from it. Section headers are
`Role::RowHeader`. Screen readers announce "row R, column C, N of M".

The grid is not a live region. A live node is announced by its *name*, which a
`Grid` takes from its label, so a live grid never said its value, and every
named tile inherited the politeness and was announced as it scrolled into the
realized window. A click, a key, an assistive click or a marquee that changes
how many tiles are selected says the new count once, through the tree's
announcer; moving a single selection says no count, since the tile a reader
lands on says it is selected.

Keyboard focus stays on the grid, which names the tile under the cursor as its
active descendant. A tile offers `Click` and `ScrollIntoView`, not `Focus`: a
screen reader's focus request on a tile moves nothing, and its click chooses
the tile and moves the cursor there. A change of selection rebuilds only the
tiles whose selectedness it flipped, below their `GridCell` nodes, so the tile
under the cursor keeps its node and its `selected` state changes in place.

`.tile_a11y_label(|index| String)` sets each `GridCell`'s accessible **name**
(e.g. `"Title, Type"`) so a screen reader announces a concise item name in
addition to the row/column position; without it the cell name is left to its
contents.

## Tests

Headless (no GPU): [crates/teksilo-widgets/src/grid_view/tests.rs](../crates/teksilo-widgets/src/grid_view/tests.rs)
plus unit tests in `layout/strategy.rs` and, for the prefix-sum table
(`layout/offsets.rs` is now only a re-export shim), `common/row_offsets.rs`. Coverage:
virtualization window, column derivation, tile placement (uniform / variable /
waterfall / sectioned), prefix-sum + anchoring, selection (by index and by
key), 2D keyboard, reorder (source `accept_drop`), type-ahead, source-driven
lazy loading (`fetch_more` + placeholder rows), the detail band (layout,
following its tile, keys, rubber band, accessibility, reveal, focus, scroll
anchoring, tile node identity, a lazy source's window loads, its visible span),
`GridViewHandle` (before mount, across rebuilds, in the same turn as opening a
band; the cursor read, set, clamped and observed), and accessibility roles. `tests/grid_view_keyed_selection_cost.rs` bounds what a
keyed selection costs as the grid grows, counting allocated bytes and key
reads rather than time.
