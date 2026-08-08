<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TreeRowMeta

Type-erased data source adapter for `TreeView`.

Wraps any `TreeDataSource` behind a uniform set of `Rc<dyn Fn(..)>` closures
keyed on the **visible flat index**, so `TreeView<T>` requires no extra type
parameter for the source's `Key`. Each closure resolves index → `Key` (via
`key_at`) before forwarding to the source's `parent`, `set_expanded`,
`can_accept`, etc. The `Key` type is fully captured here and never surfaces
in the view.

Both built-in and external backings flow through
`Rc<TreeSlice<T>>` (which implements `TreeDataSource<Key = NodeId>`), while
`TreeView::from_source` wraps an external `TreeDataSource` with its own `Key`.
The only built-in-vs-external difference — the `NodeId`-typed `TreeRowContext`
handed to the legacy delegate — lives in `tree_view.rs`, not here.

## Builder methods at a glance

`toggle_callback`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/index.html)

## `pub struct TreeRowMeta`

Key-erased per-row flat metadata, derived from the source's `FlatEntry`.

```rust
pub struct TreeRowMeta { /* fields */ }
```

## `pub struct TreeRow`

Per-row context handed to a `TreeView::from_source`
delegate — the key-erased counterpart of the built-in
`TreeRowContext`. Carries the row's flat metadata
plus a one-call chevron toggle that flips the row's expansion through the
source (by index → key → `set_expanded`).

```rust
pub struct TreeRow { /* fields */ }
```

### Methods

#### `pub fn toggle_callback(&self) -> Rc<dyn Fn(&mut EventContext)>`

Toggle callback for this row's chevron. Wires in one line:
`.on_toggle_rc(row.toggle_callback())`.
