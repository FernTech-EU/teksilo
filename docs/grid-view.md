<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# GridView — Virtualized 2D Tile Grid

`GridView<T>` is the photo-gallery / icon-view / file-manager-grid /
collection-view widget — the 2D sibling of [`ListView`](widgets-overview.md)
and `TableView`. It is bound to a `ListModel<T>` / `ListDataSource`, realizes
only the tiles currently visible (plus a buffer), reflows on resize, and is
fully keyboard-navigable and accessible.

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
| **Waterfall** | `.waterfall(estimated)` | per-item | Pinterest column-balanced flow. No scroll-anchoring (items reflow across columns). O(n) per layout on height change — fine for hundreds–low-thousands. |

**Tile sizing** ([`GridSizing`]):

- `Fixed { width, height }` — exact tile size; column count derived (tiles not stretched).
- `FixedColumnCount { count, height }` — exactly `count` stretched columns.
- `Adaptive { min_width, max_width, height }` — fit as many ≥ `min_width` columns as possible, stretch up to `max_width` (CSS `repeat(auto-fill, minmax(...))` / Flutter `maxCrossAxisExtent`).

Sugar: `.tile_size(w, h)`, `.column_count(n, h)`. Spacing: `.column_spacing`,
`.row_spacing`, `.spacing` (both), `.content_inset(EdgeInsets)`.

**Reactive sizing.** `.sizing(...)` accepts `impl Into<Prop<GridSizing>>`, so it
takes a plain `GridSizing` **or** a `Signal<GridSizing>`. A bound signal is
observed at `BindingLevel::Rebuild`: changing it rebuilds the cached layout
strategy and reflows the grid — the internal `scroll_y` / `focused_index` /
selection are field signals on the same widget instance, so they survive the
rebuild (no scroll jump). This is the "card-size slider" path — drive a
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
  1-D row widgets — `ListView` / `TreeView` / `TableView` / `TreeTableView` — from
  `common/row_offsets.rs`). After each measure pass a *realization re-check*
  compares the corrected visible range against the realized tile range and
  requests a rebuild when tiles measured shorter than the estimate would
  otherwise leave a gap at the viewport bottom — convergence is guaranteed by
  the sub-pixel measurement epsilon. When a measure pass changes the content
  total, the pane pokes the container (a `Relayout`-bound signal) so
  `max_scroll_y` and the thumb ratio — computed parent-first, before the
  measurements — are re-derived next frame; without the poke, content past the
  estimated total would stay unreachable until the next scroll.
- **Exact** (`.item_height(index)`): row heights are seeded exactly as
  `max(item_height(i))` over the row — exact scrollbar, zero jitter, no
  measurement.

Because `PrefixSumOffsets` is shared with the 1-D row widgets, a
zero-height row (`.item_height` has no floor above `0.0`) hit-tests the
same way here as it does for `TableView`/`TreeTableView`'s drop targeting —
`row_at`'s raw result is the hit-tested tile index, so see
[table-view.md "Which row a `y` coordinate resolves to"](table-view.md) for
the degenerate-height tie-break.

## Selection

Pass a flat `SelectionModel` (`None` / `Single` / `Multi`). Mouse: click =
select, Ctrl+click = toggle, Shift+click = reading-order range (Finder /
Explorer). Ctrl+A = select-all — a no-op in `Single`/`None` mode, matching
`ListView` and `TableView` (`SelectionModel::select_all` itself enforces this,
so no per-view gating is needed). `Multi` mode adds **rubber-band marquee** — a
drag on the empty background sweeps a rectangle and selects every intersecting
tile (Ctrl/Shift at drag-start = additive). The hit-test is geometric, so it
selects tiles outside the realized window. `.on_selection_changed(|set|)` fires
on every change (interactive or programmatic). `.marquee_selection(false)`
disables marquee.

## Keyboard navigation

Focus (the *current* item) is tracked separately from selection and shown by a
painted focus ring. Matrix (RTL-aware; horizontal arrows swap):

| Keys | Action |
| --- | --- |
| Arrow ←/→ | ±1 (within row; `.wrap_navigation(true)` to cross rows) |
| Arrow ↑/↓ | ±columns |
| Home / End | first / last item of the collection |
| Ctrl+Home / Ctrl+End | the same, without moving the selection |
| PageUp / PageDown | ± a viewport of rows + scroll |
| Space | check the focused tile if it holds a checkbox, else toggle (`Multi`) / select (`Single`) |
| Enter | `.on_tile_activate` (else select) |
| Esc | clear focus |
| Ctrl+A | select all (`Multi` mode only); Ctrl+Shift+A deselects |
| Alt+Arrow | reorder the focused tile (when `.reorderable`) |
| printable | type-ahead (needs `.type_ahead_label(i)`; `.type_ahead_timeout`) |
| Tab | `.tab_traversal(WithinGrid \| OutOfGrid)` |

Shift + any navigation extends the selection range. Every navigation scrolls
the new focus into view.

## Scrolling

`.scroll_y_signal()` / `.max_scroll_y_signal()` / `.viewport_ratio_y_signal()`
expose the reactive scroll state (wire an external `ScrollBar` with
`.show_scrollbar(false)` so it survives rebuilds). `.ensure_index_visible(i,
ScrollAnchor)` / `.scroll_to_index(i, ScrollAnchor)` where `ScrollAnchor` is
`Auto | Start | Center | End`. `.overscroll_behavior(Chain | Contain)` controls
scroll chaining.

## Lazy / incremental loading

There is no view-level `on_near_end` hook — incremental loading is a **source
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

`.reorderable(true)` enables intra-grid drag (and keyboard Alt+Arrow). Drags
route through the bound source's DnD capabilities: a tile is draggable only when
the source's `drag(key)` returns `CanDrag`; on hover the geometric
`(target, position)` is validated by `can_accept(query)` (a vertical insertion
bar shows an accepted landing; a rejected one suppresses it); the drop commits
via `accept_drop(commit)`. A `ListModel`-backed source moves the item by
default. `.on_item_drop(|payload, index, ctx| -> bool)` is the escape hatch for
**foreign / external** payloads (cross-view or OS drops) the source's
`can_accept` rejects — it accepts at a flat insertion index, reusing the
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

## Other

- `.on_tile_activate(|index, ctx|)` — double-click / Enter (distinct from selection).
- `.tile_context_menu(|index, pos, ctx| -> Option<Box<dyn Widget>>)` — per-tile menu.
- `.empty_view(|| ...)` when the model is empty; `.loading_view(|| ...)` + `.is_loading(signal)` for an overlaid loading state.
- RTL is honored automatically (column 0 draws at the trailing edge; horizontal arrows swap).

## Theming

`GridView` renders tiles through the app delegate, so its only widget-owned
chrome is paint-time decoration. The Tier-3 `GridViewStyle` protocol exposes
that as recipe data — focus ring (`GridFocusRingRecipe`), marquee
(`GridMarqueeRecipe`), drag-insertion bar (`GridInsertionRecipe`), and the
sticky-header surface role. Each method has a default, so a custom style
overrides only what it cares about. Precedence: `.style(...)` per-call →
`theme.style_slots.grid_view` theme-wide → the stock `RecipeGridViewStyle`.
The container itself uses theme roles (`BorderRole::Focused` / `Accent`,
`SurfaceRole::Raised`) by default.

## Accessibility

The container emits `Role::Grid` with the **logical** `row_count` /
`column_count` (not the realized window), `multiselectable` in Multi mode,
`active_descendant` pointing at the focused tile (roving focus), and a
`Live::Polite` selection-count value. Each tile is wrapped in `Role::GridCell`
with 1-based `row_index` / `column_index` and its own 1-based
`position_in_set`; the total (`size_of_set`) sits on the `Role::Grid`
container beside the row and column counts, because AccessKit resolves an
item's set size by walking *up* from it. Section headers are
`Role::RowHeader`. Screen readers announce "row R, column C — N of M" and the
selection count.

`.tile_a11y_label(|index| String)` sets each `GridCell`'s accessible **name**
(e.g. `"Title, Type"`) so a screen reader announces a concise item name in
addition to the row/column position; without it the cell name is left to its
contents.

## Tests

Headless (no GPU): [crates/teksilo-widgets/src/grid_view/tests.rs](../crates/teksilo-widgets/src/grid_view/tests.rs)
plus unit tests in `layout/offsets.rs` and `layout/strategy.rs`. Coverage:
virtualization window, column derivation, tile placement (uniform / variable /
waterfall / sectioned), prefix-sum + anchoring, selection, 2D keyboard,
reorder (source `accept_drop`), type-ahead, source-driven lazy loading
(`fetch_more` + placeholder rows), and accessibility roles.
