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
on every change (interactive or programmatic). `.marquee_selection(false)`
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
| Enter | open / close the tile's detail band (with `.detail_row`) and `.on_tile_activate` (else select) |
| ↓ on the open tile / ↑ in its band | into the band's first focusable control / back to the tile (see [Detail band](#detail-band)) |
| Esc | clear focus |
| Ctrl+A | select all (`Multi` mode only); Ctrl+Shift+A deselects |
| Alt+Arrow / Alt+Home / Alt+End | reorder the focused tile (when `.reorderable`); Alt+↑/↓ moves it a whole row |
| printable | type-ahead (needs `.type_ahead_label(i)`; `.type_ahead_timeout`) |
| Tab | `.tab_traversal(WithinGrid \| OutOfGrid)` (default `OutOfGrid`); Ctrl+Tab always leaves the grid |

Shift + any navigation extends the selection range. Every navigation scrolls
the new focus into view.

## Scrolling

`.scroll_y_signal()` / `.max_scroll_y_signal()` / `.viewport_ratio_y_signal()`
expose the reactive scroll state (wire an external `ScrollBar` with
`.show_scrollbar(false)` so it survives rebuilds). `.ensure_index_visible(i,
ScrollAnchor)` / `.scroll_to_index(i, ScrollAnchor)` where `ScrollAnchor` is
`Auto | Start | Center | End`. `.overscroll_behavior(Chain | Contain)` controls
scroll chaining.

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
    .detail_row_height(|_index| 240.0)          // optional; measured otherwise
    .tile_a11y_label(|i| album_title(i))
```

- `.detail_row(|tc| -> Option<Box<dyn Widget>>)` receives the disclosed tile's
  `TileContext`, the one the tile delegate gets, item included. `None` means
  the tile has nothing to disclose and the band takes no space.
- `.expanded_index(Signal<Option<usize>>)` is the disclosed tile. The grid
  writes it when a tile is activated (a click per `.activate_on`, or Enter):
  activating a tile opens its band, activating the open tile closes it, and
  `.on_tile_activate` still fires, after the band has opened or closed. Writing
  the signal from outside opens and closes the band. Without it the grid keeps
  a signal of its own.
- `.detail_row_height(|index| -> f32)` sizes the band. Without it the band is
  measured height-for-width at the content width (the viewport less its side
  insets), as the variable-height tiles are, with the same scroll anchoring
  when its height changes above the viewport top.

**Layout.** The band sits one row gap under the open row, and everything after
it moves down by the band and a row gap: in the realized window, in
`max_scroll_y`, in the focus ring, the drop indicator and every scroll-into-view.
It is not a row of the grid: tiles keep their row and column numbers. It works
with the uniform grid, `.variable_row_heights` / `.item_height` (the band goes
under the row's tallest tile) and `.sections` (it opens inside its section and
the following headers move down). A `.waterfall` has no rows, so the band is
ignored there.

**Following the tile.** An insert, a removal or a move keeps the band on its
tile, and removing the tile closes it. A reset (a `SortFilterListModel` sort or
filter, `ListModel::replace_all`) keeps it on its tile when the source has item
keys, and closes it when the source has none or the tile is gone. A `ListModel`'s
keys are its positions, which say nothing about where an item went, so a
`ListModel` closes the band on a reset whether it is given to `new`,
`from_source` or `from_source_keyed`.

**Keyboard.** Enter or Space on the open tile closes it. ↓ from the open tile
moves focus to the first focusable control in the band; ↑ there, when the
control does not use it, returns focus to the grid with the cursor on the tile.
A band with nothing focusable is stepped over like any other key. Arrows
between tiles step over the band, and while focus is in the band the grid's own
keys stand aside, so a key the band's content lets through moves no tile.

**Rubber band.** The marquee selects tiles where they are, so it never selects
the band, and a press on the band starts no marquee.

**Accessibility.** The band is a `Role::Group` named after its tile's
`.tile_a11y_label` (labelled by the tile node otherwise). Every tile publishes
`expanded`, offers `Expand` or `Collapse` (which open and close the band for a
screen reader, with no double click needed), and, while open, a `controls`
relation to the band. The grid's row and column counts and each tile's position
in the set are unchanged. Opening and closing replace no tile node.

**Lifetime.** The band widget is a child of the grid, not of the body pane, so
scrolling and resizing keep its content (a track list's own scroll and focus).
The content is rebuilt when the band moves to another tile, and when the grid's
data changes.

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
following its tile, keys, rubber band, accessibility), and accessibility roles.
