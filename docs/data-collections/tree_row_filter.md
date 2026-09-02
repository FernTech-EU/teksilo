<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TreeRowFilter

`TreeRowFilter` — sort + tree-aware filter over a `TreeRow` stream.

The composable sort/filter stage for the `TreeDataSlice`
pipeline. Where `SortFilterTreeModel` is a full
projection *over an in-memory `TreeModel`* (it owns its own expand state), an
external tree already has its expand/flatten projection — the
`TreeDataSlice`. Stacking a second projection on top would mean two expand
states. So for external trees, sort/filter belongs **below** the slice, as a
transform of its raw indent-ordered input:

```text
rows::load()  →  TreeRowFilter::apply  →  TreeDataSlice::set_source  →  TreeView
              \___ Vec<TreeRow> → Vec<TreeRow> ___/     \___ the one projection ___/
```

It uses the same three `TreeFilterMode` strategies and sorts siblings per
parent, then re-emits a valid indent-ordered stream (surviving nodes' depths
are compacted onto their nearest surviving ancestor, which `TreeDataSlice`
re-derives into a clean tree):

- **`KeepAncestors`** — a node stays if it matches or any descendant matches
  (the outline-search behaviour; equivalent to `SortFilterTreeModel`).
- **`HideNonMatching`** — a node stays only if it *and every ancestor* match
  (children of a hidden parent stay hidden; equivalent to `SortFilterTreeModel`).
- **`KeepDescendants`** — a match keeps its whole subtree, surfaced even when
  the match's own ancestors don't match (the subtree compacts onto a root).
  This deliberately differs from `SortFilterTreeModel`, whose flatten drops a
  match unless its full ancestor path is visible — which defeats the mode's
  "keep the match and its subtree" intent.

## Revealing the matches

`TreeRowFilter` reshapes the *rows*; it does not touch the slice's per-view
**expand state**. So `KeepAncestors` keeps the ancestor rows, but a
freshly-collapsed `TreeDataSlice` still hides the matches under them. While a
filter is active, flip the slice's reveal override so the whole narrowed
result shows; turn it off when the filter clears (the user's real collapse
state is preserved underneath):

```ignore
let filtered = !query.is_empty();
slice.set_source(move || if filtered { sieve.apply(load()) } else { load() });
slice.reload();
slice.set_all_expanded(filtered);   // reveal while searching, restore after
```

## Example

```
use teksilo_data::{TreeRowFilter, TreeRow, TreeFilterMode};

let rows = vec![
    TreeRow::new(1u64, "Book One", 0),
    TreeRow::new(2, "Opening", 1),
    TreeRow::new(3, "The Dawn Raid", 1),
    TreeRow::new(4, "Notes", 0),
];

// Outline search: keep matches and the folders that lead to them.
let sieve = TreeRowFilter::new()
    .filter_mode(TreeFilterMode::KeepAncestors)
    .filter(|title: &&str| title.contains("Dawn"));
let out = sieve.apply(rows);
// "Book One" (ancestor of the match) + "The Dawn Raid".
assert_eq!(out.iter().map(|r| r.item).collect::<Vec<_>>(), vec!["Book One", "The Dawn Raid"]);
```

## Builder methods at a glance

`filter_mode`, `filter`, `sort`, `sort_desc`, `apply`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/tree_row_filter/index.html)

## `pub struct TreeRowFilter`

A reusable sort + tree-aware filter over a `Vec<``TreeRow``<K, T>>`. Build
it once, `apply` it to each freshly-sourced row stream (e.g.
inside a `TreeDataSlice::set_source` closure). See the `module docs`.

```rust
pub struct TreeRowFilter<K: ItemKey, T> { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

An identity transform (no filter, no sort). Chain `filter`
/ `sort` to configure it.

#### `pub fn filter_mode(mut self, mode: TreeFilterMode) -> Self`

Set the filter strategy (how ancestors/descendants of a match are kept).
Defaults to `TreeFilterMode::default()`.

#### `pub fn filter(mut self, pred: impl Fn(&T) -> bool + 'static) -> Self`

Set the match predicate over the row item. A row "matches" when `pred`
returns `true`; the `filter_mode` decides what else
stays visible. With no predicate every row is kept.

#### `pub fn sort(mut self, cmp: impl Fn(&T, &T) -> Ordering + 'static) -> Self`

Sort siblings (ascending) by a comparator on the row item. Parent/child
structure is preserved — only the order within each parent changes.

#### `pub fn sort_desc(mut self, cmp: impl Fn(&T, &T) -> Ordering + 'static) -> Self`

Sort siblings (descending) by a comparator on the row item.

#### `pub fn apply(&self, rows: Vec<TreeRow<K, T>>) -> Vec<TreeRow<K, T>>`

Apply the filter + sort to an indent-ordered row stream, returning a new
indent-ordered stream. `O(n log n)` for the sort, `O(n)` otherwise.
