<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Data Sources — the read-and-command interface

The four data views — `ListView`, `TreeView`, `TableView`, `TreeTableView` —
do **not** own a private store they mutate. They read from, and *command*, a
**data source**: a trait the application implements (or reuses a built-in impl
of) that owns the truth. This is Bastyde's answer to Qt's `QAbstractItemModel`
capability protocol — `flags` / `canDropMimeData` / `dropMimeData` for drag-and-
drop, `canFetchMore` / `fetchMore` for lazy loading — but expressed as
**defaulted methods on concrete-`T` source traits**, not a type-erased
`QVariant`/`QModelIndex` base class. A view reads `&T` directly; the source
answers "may this drop happen?" and "apply it"; the view only renders the
verdict and routes the commit.

There are two traits, both in `bastyde-data`:

| Trait | Shape | Built-in impls |
| --- | --- | --- |
| [`ListDataSource`](../crates/bastyde-data/src/list_data_source.rs) | flat list | `ListModel<T>`, `SortFilterListModel<T>` |
| [`TreeDataSource`](../crates/bastyde-data/src/tree_data_source.rs) | per-view flattened tree | `TreeSlice<T>`, `SortFilterTreeModel<T>` |

Neither is object-safe (associated types + generic `with_item`/`with_entry`),
so a view consumes it generically via `from_source(...)` and erases it into an
internal closure bundle — the view type stays `ListView<T>` / `TreeView<T>`,
**not** `ListView<T, S>`. The `Key` is captured at the `from_source` boundary,
so it never leaks into the view's type parameters.

> **When to implement a source vs. use a built-in model.** A bounded, in-memory
> collection that the view-model owns is a `ListModel` / `TreeModel` (which
> *are* sources — see the matrix below); reach for them first. Implement a
> source trait **directly** when the truth lives elsewhere (a DB cursor, a
> Qleany entity store, a paged feed) or doesn't fit in memory. Then there is no
> second copy to keep in sync. See [data-models.md §14](data-models.md).

---

## 1. The core read surface

### `ListDataSource`

```rust
pub trait ListDataSource: 'static {
    type Item: 'static;
    type Key: ItemKey;                                   // identity

    fn len(&self) -> usize;                              // TOTAL (incl. not-yet-loaded)
    fn with_item<R>(&self, index: usize, f: impl FnOnce(&Self::Item) -> R) -> Option<R>;
    fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle;

    // defaulted ↓
    fn is_empty(&self) -> bool { self.len() == 0 }
    fn key_at(&self, index: usize) -> Option<Self::Key> { None }
    fn index_of(&self, key: &Self::Key) -> Option<usize> { None }
    fn first_changed_index(&self) -> Option<usize> { None }
    // … DnD + lazy capability methods (§3, §4) …
}
```

A read-only in-memory source implements only `len` + `with_item` +
`observe_changes`. `with_item` returns `None` for an out-of-bounds index **or**
an in-bounds index whose data isn't resident yet (a lazy window miss — §4).

### `TreeDataSource`

A tree source exposes its **per-view flattened, currently-visible** rows (the
`TreeSlice` shape — expand state is per view, so two `TreeView`s over one source
have independent expansion):

```rust
pub trait TreeDataSource: 'static {
    type Item: 'static;
    type Key: ItemKey;

    fn visible_count(&self) -> usize;
    fn with_entry<R>(&self, flat_index: usize,
        f: impl FnOnce(&Self::Item, &FlatEntry<Self::Key>) -> R) -> Option<R>;
    fn key_at(&self, flat_index: usize) -> Option<Self::Key>;
    fn flat_index_of(&self, key: &Self::Key) -> Option<usize>;
    fn parent(&self, key: &Self::Key) -> Option<Self::Key>;     // sibling nav + cycle guard
    fn child_keys(&self, key: &Self::Key) -> Vec<Self::Key>;
    fn version_signal(&self) -> Signal<u64>;                    // bound at Rebuild
    fn is_expanded(&self, key: &Self::Key) -> bool;
    fn set_expanded(&self, key: &Self::Key, expanded: bool);

    // defaulted ↓
    fn first_changed_index(&self) -> Option<usize> { None }
    fn contains_key(&self, key: &Self::Key) -> bool { self.flat_index_of(key).is_some() }
    // … DnD + lazy capability methods (§3, §4) …
}
```

`FlatEntry<K = NodeId> { node_id: K, depth, has_children, is_expanded }` carries
the per-row tree metadata the delegate needs. Trees drive the view off
`version_signal()` (bumps on every structural/projection change) plus
`first_changed_index()` (the divergence prefix that lets row-height caches and
other per-row state survive a reflatten).

`contains_key` is **visibility-independent**: a node collapsed under an ancestor
(or scrolled out of a lazy window) still exists. It's what keyed-selection
pruning consults, so a collapsed-but-present node keeps its selection and only a
*deleted* node is dropped. The default is visible-only — an external store whose
nodes persist while collapsed should override it.

---

## 2. Wiring a view

```rust
// Flat:
let list = ListView::from_source(source, |index, item: &Row, selected| {
    Box::new(StandardListItem::new(item.title.clone()).selected(selected))
});

// Tree: the delegate gets a TreeRow (depth / has_children / is_expanded +
// a one-call chevron `toggle_callback()`):
let tree = TreeView::from_source(source, |item: &Node, row, selected| {
    Box::new(StandardTreeItem::new(item.name.clone())
        .from_entry(row)
        .on_toggle_rc(row.toggle_callback()))
});
```

The built-in models implement the trait, so `ListView::from_source(list_model,
…)` / `TreeView::from_source(tree_slice, …)` work unchanged; `from_model` sugar
exists where it reads better. `TableView::from_source(source)` then adds columns
via the builder; `TreeTableView` fuses a `TreeDataSource` with columns. See
[table-view.md](table-view.md).

---

## 3. Capability — drag-and-drop validation

The source owns DnD. The view never paints an "always valid" insertion line; it
asks the source on every hover and refuses a rejected drop. The vocabulary
([`dnd_types`](../crates/bastyde-data/src/dnd_types.rs)) is shared by all four
data views (and adapted by `TabBar`):

```rust
enum DragEligibility { CanDrag, NoDrag }                  // the transferable gate
enum DragSource<'a, K> { SameView { key: K }, Foreign { payload: &'a DragPayload } }
struct DropQuery<'a, K>  { source: DragSource<'a, K>, target: K, position: DropPosition }
enum   DropResponse      { Accept, Reject, Redirect(DropPosition) }
struct DropCommit<'a, K> { source: DragSource<'a, K>, target: K, position: DropPosition }
enum   DropPosition      { Before, Into, After }          // Into = reparent (trees only)
```

The four source methods (all default to inert):

```rust
fn drag(&self, key: &Self::Key) -> DragEligibility;       // may this row start a drag?
fn can_accept(&self, q: &DropQuery<'_, Self::Key>) -> DropResponse;   // hover verdict
fn accept_drop(&self, c: DropCommit<'_, Self::Key>) -> bool;          // apply
fn on_drag_out(&self, key: &Self::Key);                   // source-side completion
```

**Flow per drag:**

1. **Start.** A row begins a drag only if `drag(key) == CanDrag`. The view emits
   a `RowDrag { source_index, source_view_id }` typed payload (shared by all
   four views via `data_views`).
2. **Hover.** The view computes the geometric `(target_key, position)` and calls
   `can_accept`. `Accept` paints the insertion line / reparent box; `Reject`
   paints the no-drop affordance and will refuse; `Redirect(pos)` snaps the
   indicator to `pos` (e.g. a container that takes children but not sibling
   reorder redirects `Before`/`After` → `Into`). **This is the pre-drop
   validation the integrated reorder never had.**
3. **Drop.** The view re-queries `can_accept`; if not `Reject`, it calls
   `accept_drop(commit)`. A store-backed source mutates its `Vec`/arena; an
   external source routes to its command (a Qleany `move_node`, an SQL `UPDATE`).
4. **Cross-view / external.** A drag from another view or the OS arrives as
   `DragSource::Foreign { payload }`; the source downcasts the payload itself
   (`payload.get_typed::<MyPaletteDrop>()`, `payload.files()`, …). The same
   `can_accept` / `accept_drop` path covers intra-view reorder, list→list
   transfer, palette→outline drop, and OS file drops — one protocol, not a bolt-
   on.
5. **Source-side completion.** After a `Foreign` drop is accepted *elsewhere*,
   the framework calls `on_drag_out(key)` on the **origin** source. A
   shared/command-backed source no-ops it; an independent model uses it to drop
   the moved row. (Same-view reorders don't fire it.)

**Keyboard reorder.** `Alt`+`Arrow` synthesizes the same `RowDrag` against the
sibling target derived from `parent`/`child_keys`, then routes through
`can_accept` → `accept_drop`. All four data views share this (TableView /
TreeTableView gained it in the redesign).

**Tree reorder helpers.** Custom `TreeDataSource` impls building on a
`TreeModel` can reuse
[`tree_apply_reorder`](../crates/bastyde-data/src/tree_data_source.rs) (applies a
`(source, target, position)` move with the remove-then-insert index adjustment)
and `tree_is_desc_or_self` (the cycle guard — you cannot drop a node into its
own subtree). The built-in `TreeSlice` / `SortFilterTreeModel` `accept_drop`
impls are built on them.

---

## 4. Capability — lazy / windowed loading

There is **no** view-level `on_near_end` hook — incremental loading is a source
capability (all defaulted to fully-resident):

```rust
enum RowState { Ready, Loading }

fn row_state(&self, index: usize) -> RowState;            // is this row resident?
fn request_window(&self, range: Range<usize>);           // load the visible+buffer window
fn can_fetch_more(&self) -> bool;                        // append-only growth available?
fn fetch_more(&self);                                    // pull the next page
```

Each realize pass the view calls `request_window(start..end)` for its visible +
buffer range, and when the scroll nears the end consults `can_fetch_more()` →
`fetch_more()`. A row whose `with_item`/`with_entry` returns `None` **and** whose
`row_state(i) == Loading` is rendered as a **placeholder skeleton** at the row's
estimated height instead of being skipped — so selection, focus, and scroll math
stay stable while the page loads. A `Loading` row keeps its `PrefixSumOffsets`
estimate (it is never `set_row_height`-ed), so layout doesn't jump.

Two shapes are supported:

- **Windowed** (total known, sliding resident window — the 1M-row DB): `len()` /
  `visible_count()` returns the total; `row_state` is `Loading` outside the
  resident window; `request_window` slides the window.
- **Append** (total unknown — infinite scroll): `can_fetch_more` / `fetch_more`
  grow the source.

When a page lands, a flat source emits `DataChange::WindowLoaded { range }` — a
variant **distinct** from `ItemsInserted` so index-based `SelectionModel` does
**not** index-shift (the rows already existed; only their data arrived); the
divergence prefix is `range.start`. Trees need no new variant — a
`version_signal()` bump + `first_changed_index()` cover it. The page fetch itself
runs off-thread / on the app executor and updates the resident buffer on the
main thread (the existing `AsyncCompletionHandle` machinery); the next realize
swaps placeholders for real rows and `place_children` re-measures.

---

## 5. Keyed selection

Index-based `SelectionModel` is unstable under windowing and external reorder (a
position means a different row after a slide). `KeyedSelectionModel<K>` stores a
`HashSet<K>` keyed by source identity, plus the range anchor **as a key**, so
selection survives reorders, filters, window-slides, and stays consistent across
two views of one source.

```rust
let keyed = KeyedSelectionModel::new(SelectionMode::Multi);
let list = ListView::from_source_keyed(source, keyed.clone(), delegate);
// keyed.select(k) / .toggle(k) / .extend_to(target, &ordered_visible_keys)
// .is_selected(&k) / .selected_keys() / .selection_signal()
```

The view resolves `is_selected(source.key_at(i))` at the realization loop and
`select(key)` in the click handler. On `ItemsRemoved` / `Reset` it calls
`prune_missing(|k| source has k)` — for trees that consults `contains_key`, so a
collapsed-but-present node keeps its selection. `extend_to` ranges over the
current visible key order; an anchor scrolled out gracefully degrades to single
select. `from_source_keyed` (all four views) opts in; plain `from_source` keeps
index selection.

---

## 6. Built-in implementation matrix

| Type | Trait | `Key` | DnD | Lazy |
| --- | --- | --- | --- | --- |
| `ListModel<T>` | `ListDataSource` | `usize` | `accept_drop` = `move_item`, `can_accept` = `Accept` | resident |
| `SortFilterListModel<T>` | `ListDataSource` | `usize` | inert (sorted view) | resident |
| `TreeSlice<T>` | `TreeDataSource` | `NodeId` | `accept_drop` = `move_node` w/ cycle guard | resident |
| `SortFilterTreeModel<T>` | `TreeDataSource` | `NodeId` | as `TreeSlice` | resident |

`TreeModel<T>` is **not** itself a `TreeDataSource` — it carries no per-view
expand state; wrap it in a `TreeSlice` (independent expansion per view) or a
`SortFilterTreeModel`. External sources supply their own `Key` (an `i64` entity
id, a `Uuid`, …) and implement the trait directly — that domain key is exactly
what removes the need for a mirror model.

---

## See also

- [data-models.md](data-models.md) — the built-in models, projections, the
  `first_changed_index()` divergence side-channel (§13), and projecting an
  external source of truth (§14).
- [table-view.md](table-view.md) — `TableView` / `TreeTableView` columns, row
  heights, and source binding.
- [grid-view.md](grid-view.md) — `GridView` rides the same `ListDataSource`
  capabilities (drag routing + `fetch_more` + placeholders).
- [drag-and-drop.md](drag-and-drop.md) — the framework DnD pipeline the source
  protocol routes through, `DropTarget`/`DropZone`, and drop-target bubbling.
- Source: [list_data_source.rs](../crates/bastyde-data/src/list_data_source.rs),
  [tree_data_source.rs](../crates/bastyde-data/src/tree_data_source.rs),
  [dnd_types.rs](../crates/bastyde-data/src/dnd_types.rs),
  [keyed_selection_model.rs](../crates/bastyde-data/src/keyed_selection_model.rs).
