<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# GridView

Virtualized 2D tile grid bound to a `ListModel<T>` / `ListDataSource`.

`GridView` is the photo-gallery / icon-view / file-manager-grid /
collection-view widget — the 2D sibling of `ListView`
and `TableView`. It realizes only the
tiles currently visible (plus a buffer), reflows on resize, supports
single / multi selection with 2D keyboard navigation, and is fully
accessible (`Role::Grid` → `Role::GridCell`).

The layout is pluggable via `GridLayoutStrategy`;
the stock `UniformGrid` gives fixed tile size /
fixed column count / adaptive min-width grids. (Variable-row-height and
waterfall strategies, plus marquee selection, drag-reorder, sections and
sticky headers, are layered on in later phases.)

```ignore
GridView::new(model, |tc| {
    Box::new(Card::new().child(TextWidget::new(lit!(&tc.item.name))))
})
.sizing(GridSizing::Adaptive { min_width: 120.0, max_width: None, height: 140.0 })
.spacing(8.0)
.selection(selection_model)
```

## Builder methods at a glance

`from_source`, `enabled`, `sizing`, `tile_size`, `column_count`, `variable_row_heights`, `item_height`, `waterfall`, `column_spacing`, `row_spacing`, `spacing`, `content_inset`, `selection`, `on_selection_changed`, `marquee_selection`, `wrap_navigation`, `tab_traversal`, `show_scrollbar`, `overscroll_behavior`, `smooth_scrolling`, `smooth_scroll_duration`, `scroll_bar_style`, `scroll_y_signal`, `max_scroll_y_signal`, `viewport_ratio_y_signal`, `ensure_index_visible`, `scroll_to_index`, `sections`, `section_header_delegate`, `section_header_height`, `pinned_section_headers`, `a11y_label`, `style`, `empty_view`, `loading_view`, `is_loading`, `reorderable`, `on_item_drop`, `on_tile_activate`, `activate_on`, `tile_context_menu`, `type_ahead_label`, `type_ahead_timeout`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/grid_view/index.html)

## `pub struct TileContext`

Context passed to the tile delegate for each realized tile.

Richer than `ListView`'s `(index, &item, selected)` — carries the 2D
grid coordinates and focus state (mirrors `TableView`'s `CellContext`).
There is intentionally **no** `is_hovered`: hover changes on every
mouse-move and is handled per-tile inside the delegate's own widget
(its interaction signal), never by rebuilding the grid.

```rust
pub struct TileContext<'a, T: 'static> { /* fields */ }
```

## `pub struct GridView`

A virtualized 2D tile grid backed by a `ListModel<T>`.

```rust
pub struct GridView<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new( model: ListModel<T>, delegate: impl Fn(&TileContext<'_, T>) -> Box<dyn Widget> + 'static, ) -> Self`

Create a grid backed by a `ListModel<T>`. The `delegate` builds the
widget for each tile from a `TileContext`.

#### `pub fn from_source<S: bastyde_data::ListDataSource<Item = T>>( source: S, delegate: impl Fn(&TileContext<'_, T>) -> Box<dyn Widget> + 'static, ) -> Self`

Create a grid backed by any `ListDataSource` (large / external data).

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Enable or disable the whole view. A disabled view greys out and stops
accepting focus / selection / keyboard input (arena-gated).

#### `pub fn sizing(mut self, sizing: GridSizing) -> Self`

Set the tile sizing / column-count policy.

#### `pub fn tile_size(mut self, width: f32, height: f32) -> Self`

Sugar for `GridSizing::Fixed` — every tile is exactly `width` × `height`.

#### `pub fn column_count(mut self, count: usize, tile_height: f32) -> Self`

Sugar for `GridSizing::FixedColumnCount` — exactly `count` columns.

#### `pub fn variable_row_heights(mut self, estimated: f32) -> Self`

Switch to variable row heights: each row is sized to its tallest
tile (SwiftUI `LazyVGrid` semantics). `estimated` seeds rows that
haven't been measured yet; the scroll position is anchored when an
estimate is later corrected. Combine with
`item_height` for exact heights.

#### `pub fn item_height(mut self, f: impl Fn(usize) -> f32 + 'static) -> Self`

Supply an exact per-**item** natural height. Width-independent, so it
doesn't depend on the runtime column count: `VariableRowGrid` sizes
each row to `max(item_height(i))` over its items. Implies variable row
heights, gives an exact scrollbar, and removes anchoring jitter.

#### `pub fn waterfall(mut self, estimated: f32) -> Self`

Switch to a Pinterest-style waterfall: per-item variable heights flow
into the currently-shortest column. Column count comes from the
configured `sizing`; heights are auto-measured (or
exact via `item_height`). `estimated` seeds
unmeasured items.

#### `pub fn column_spacing(mut self, spacing: f32) -> Self`

Horizontal gap between tiles (default 8).

#### `pub fn row_spacing(mut self, spacing: f32) -> Self`

Vertical gap between tile rows (default 8).

#### `pub fn spacing(mut self, spacing: f32) -> Self`

Set both column and row spacing.

#### `pub fn content_inset(mut self, inset: EdgeInsets) -> Self`

Inset from the scroll-content edge to the tiles.

#### `pub fn selection(mut self, sel: SelectionModel) -> Self`

Set the selection model (modes `None` / `Single` / `Multi`).

#### `pub fn on_selection_changed(mut self, f: impl Fn(&BTreeSet<usize>) + 'static) -> Self`

Called whenever the selection set changes — including programmatic
changes — with the new set of selected indices.

#### `pub fn marquee_selection(mut self, enabled: bool) -> Self`

Enable / disable rubber-band marquee selection (default enabled; only
active when the selection model is in `Multi` mode).

#### `pub fn wrap_navigation(mut self, enabled: bool) -> Self`

Whether arrow navigation wraps across row/grid edges (default false).

#### `pub fn tab_traversal(mut self, traversal: GridTabTraversal) -> Self`

How Tab moves out of (or within) the grid (default `OutOfGrid`).

#### `pub fn show_scrollbar(mut self, show: bool) -> Self`

Suppress the internal scrollbar (mount your own via the signal
accessors so it survives rebuilds).

#### `pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self`

Scroll-chaining behavior at the boundary (default `Chain`).

#### `pub fn smooth_scrolling(mut self, enabled: bool) -> Self`

Enable or disable animated wheel scrolling (enabled by default).

#### `pub fn smooth_scroll_duration(mut self, duration: Duration) -> Self`

Duration of the smooth scroll animation (default 150 ms).

#### `pub fn scroll_bar_style(mut self, style: ScrollBarMode) -> Self`

How the scroll bar is displayed (default `Permanent`). `Overlay`
and `Thin` float the bar over the content instead of reserving a
layout column, mirroring `ScrollArea::scroll_bar_style`.

#### `pub fn scroll_y_signal(&self) -> &Signal<f32>`

The vertical scroll offset signal.

#### `pub fn max_scroll_y_signal(&self) -> &Signal<f32>`

The maximum scroll offset signal (`content_height - viewport_height`).

#### `pub fn viewport_ratio_y_signal(&self) -> &Signal<f32>`

The vertical viewport-to-content ratio signal (drives the thumb size).

#### `pub fn ensure_index_visible(&self, index: usize, anchor: ScrollAnchor)`

Scroll the minimum distance to bring `index` into view per `anchor`.

#### `pub fn scroll_to_index(&self, index: usize, anchor: ScrollAnchor)`

Scroll to `index`, forcing the viewport position per `anchor`
(`Auto` behaves like `ensure_index_visible`).

#### `pub fn sections<P: SectionProvider>(mut self, provider: P) -> Self`

Group the flat model into sections, rendering a header above each
section's tile band. Sections compose with the uniform tile layout.

#### `pub fn section_header_delegate( mut self, f: impl Fn(usize, &str) -> Box<dyn Widget> + 'static, ) -> Self`

Custom section-header widget builder `(section_index, title)`. Without
it a default bold-text header is used.

#### `pub fn section_header_height(mut self, height: f32) -> Self`

Height of each section header row (default 28).

#### `pub fn pinned_section_headers(mut self, enabled: bool) -> Self`

Keep the current section's header pinned to the top while scrolling
through it (SwiftUI `pinnedViews:[.sectionHeaders]`).

#### `pub fn a11y_label(mut self, label: impl Into<String>) -> Self`

Accessible label for the grid container.

#### `pub fn style(mut self, style: impl GridViewStyle) -> Self`

Per-call Tier-3 decoration style override (focus ring, marquee,
insertion bar, pinned-header surface). Precedence: this override →
`theme.style_slots.grid_view` → the stock `RecipeGridViewStyle`.

#### `pub fn empty_view(mut self, f: impl Fn() -> Box<dyn Widget> + 'static) -> Self`

Widget shown when the model is empty.

#### `pub fn loading_view(mut self, f: impl Fn() -> Box<dyn Widget> + 'static) -> Self`

Widget overlaid while `is_loading` reads `true`.

#### `pub fn is_loading(mut self, flag: impl Into<Prop<bool>>) -> Self`

Reactive loading flag; when `true` the `loading_view`
is shown above the grid.

#### `pub fn reorderable(mut self, enabled: bool) -> Self`

Enable intra-grid drag reordering (and keyboard Alt+Arrow). The move is
routed through the source's `accept_drop` (a built-in `ListModel`
reorders via `move_item`; an external source applies its own command).

#### `pub fn on_item_drop( mut self, f: impl Fn( bastyde_core::drag_payload::DragPayload, usize, &mut bastyde_core::widget::EventContext, ) -> bool + 'static, ) -> Self`

Accept external drops at a flat insertion index. Returns `true` when
the drop is accepted.

#### `pub fn on_tile_activate( mut self, f: impl Fn(usize, &mut bastyde_core::widget::EventContext) + 'static, ) -> Self`

Called when a tile is activated (a click per `activate_on`,
or Enter on the focused tile) — the "open / default action", distinct
from selection.

#### `pub fn activate_on(mut self, mode: crate::data_views::ActivateOn) -> Self`

Choose single- vs double-click tile activation (default
`ActivateOn::DoubleClick`). Enter activates in either
mode.

#### `pub fn tile_context_menu( mut self, f: impl Fn(usize, Point, &mut bastyde_core::widget::EventContext) -> Option<Box<dyn Widget>> + 'static, ) -> Self`

Per-tile context-menu factory: `(index, pointer_position, ctx)` →
optional menu widget.

#### `pub fn type_ahead_label(mut self, f: impl Fn(usize) -> String + 'static) -> Self`

Supply a per-item label for type-ahead navigation (typing letters
jumps to the first matching item). Required to enable type-ahead.

#### `pub fn type_ahead_timeout(mut self, timeout: std::time::Duration) -> Self`

Type-ahead reset timeout (default 500 ms; `ZERO` disables).
