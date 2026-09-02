<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Grid

![Grid preview](img/grid.png)

Grid — a 2D layout container with explicit row and column tracks.

Columns and rows are declared as `TrackSize` slices supporting three
sizing modes: `Fixed(px)` (exact logical pixels), `Auto` (sized to the
largest child in that track), and `Fractional(fr)` (share of the remaining
space after fixed and auto tracks are allocated — the CSS `fr` unit).
Children are placed in **row-major order**: child 0 occupies cell
`(row=0, col=0)`, child 1 `(row=0, col=1)`, and so on. Dormant children
are excluded from placement while keeping their siblings at their original
cell positions, so toggling a cell visible/dormant does not shift other
cells.

Fractional columns fall back to the child's natural width when the parent
provides no width constraint (intrinsic-measurement pass), preventing
wrap-aware children from reporting inflated heights.

```rust
# use teksilo_widgets::primitives::{Grid, TrackSize, RectWidget};
// Two equal columns with a fixed 40 dp row, separated by an 8 dp gap
let _grid = Grid::new()
    .columns(vec![TrackSize::Fractional(1.0), TrackSize::Fractional(1.0)])
    .rows(vec![TrackSize::Fixed(40.0)])
    .column_gap(8.0)
    .child(RectWidget::new())
    .child(RectWidget::new());
```

## Builder methods at a glance

`columns`, `rows`, `column_gap`, `row_gap`, `add_child`, `child`, `children`, `child_opt`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/grid/index.html)

## `pub enum TrackSize`

Sizing mode for a single row or column track in a `Grid`.

```rust
pub enum TrackSize { /* variants */ }
```

### Variants

- **`Fixed`** — Fixed size in logical pixels regardless of available space.
- **`Fractional`** — Share of the remaining space after `Fixed` and `Auto` tracks are resolved; equivalent to the CSS `fr` unit.  Multiple `Fractional` tracks divide the remainder proportionally to their weights.
- **`Auto`** — Sized to the largest intrinsic dimension among all children in the track; expands to fill content, never clips.

## `pub struct Grid`

A 2D grid layout container with explicit track declarations.

```rust
pub struct Grid { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create a new `Grid` with a single `Auto` column and a single `Auto`
row; configure track definitions with `columns` and
`rows`.

#### `pub fn columns(mut self, columns: Vec<TrackSize>) -> Self`

Set the column track definitions; each entry describes one column's
sizing mode.

#### `pub fn rows(mut self, rows: Vec<TrackSize>) -> Self`

Set the row track definitions; each entry describes one row's sizing
mode.

#### `pub fn column_gap(mut self, gap: impl Into<Prop<f32>>) -> Self`

Set the inter-column gap. Accepts static `f32` or `Signal<f32>`.

#### `pub fn row_gap(mut self, gap: impl Into<Prop<f32>>) -> Self`

Set the inter-row gap. Accepts static `f32` or `Signal<f32>`.

#### `pub fn add_child(mut self, id: WidgetId) -> Self`

Append a pre-registered child by ID; children are placed in row-major
order starting at `(row=0, col=0)`.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Append an inline child widget in the next cell (row-major order).

#### `pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self`

Append multiple inline children from an iterator, each occupying the
next cell in row-major order.

#### `pub fn child_opt(mut self, widget: Option<impl Widget + 'static>) -> Self`

Append an optional inline child; a `None` value is a no-op, keeping
subsequent children at their original cell positions.
