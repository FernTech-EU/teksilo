<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TreeView

TreeView — a virtualized, expandable/collapsible hierarchical list widget.

Displays a `TreeModel<T>` as an indented tree.
Internally each view owns a `TreeSlice` for independent
expand state, so two `TreeView`s on the same model can be open at different
depths simultaneously. Only rows in the visible viewport + a small buffer have
live widgets — rows outside the buffer are dormant, matching `ListView`'s
virtualization model. An external `TreeDataSource`
is also accepted via `TreeView::from_source` when the data lives outside a
`TreeModel`.

Row heights come in three modes: uniform (`item_height`, default fast path),
exact per-flat-index callback (`item_height_fn`), and auto-measured
(`auto_item_height` — height-for-width per row, scroll-anchored).

## Example

```rust
# use bastyde_widgets::TreeView;
# use bastyde_widgets::primitives::{HStack, Padding, TextWidget};
# use bastyde_data::TreeModel;
# use bastyde_i18n::lit;
# struct Item { title: String }
# let tree_model: TreeModel<Item> = TreeModel::new();
let _w = TreeView::new(tree_model, |item, entry, _selected| {
    let indent = entry.depth as f32 * 20.0;
    Box::new(HStack::new()
        .child(Padding::new(0.0, 0.0, 0.0, indent))
        .child(TextWidget::new(lit!(&item.title))))
})
.item_height(28.0);
```

## Builder methods at a glance

`new_with_context`, `from_source`, `from_source_keyed`, `overscroll_behavior`, `item_height`, `smooth_scrolling`, `smooth_scroll_duration`, `scroll_bar_style`, `item_height_fn`, `auto_item_height`, `row_click_expands`, `selection`, `keyed_selection`, `reorderable`, `on_activate`, `activate_on`, `type_ahead_label`, `type_ahead_timeout`, `expand`, `collapse`, `toggle`, `expand_all`, `collapse_all`, `tree_slice`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/tree_view/index.html)

## `pub struct TreeRowContext`

Per-row context passed to a 4-arg TreeView delegate. Carries a
reference to the slice handle and the row's `NodeId` so the
delegate can wire chevron toggles and other tree-aware behavior
without manually cloning state outside the closure.

Created internally by `TreeView::new_with_context`. Not
constructed directly by user code.

```rust
pub struct TreeRowContext<'a, T: 'static> { /* fields */ }
```

### Methods

#### `pub fn toggle_callback(&self) -> std::rc::Rc<dyn Fn(&mut bastyde_core::widget::EventContext)>`

Toggle callback for this row's chevron. Wires in one line:
`.on_toggle_rc(ctx.toggle_callback())`.

#### `pub fn slice_handle(&self) -> TreeSliceHandle<T>`

Cloned handle to the slice — call `.toggle_expand(node)`,
`.expand(node)`, `.collapse(node)` directly.

#### `pub fn node_id(&self) -> bastyde_data::NodeId`

The `NodeId` of this row in the backing `TreeModel`.

## `pub struct TreeView`

```rust
pub struct TreeView<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new( model: TreeModel<T>, delegate: impl Fn(&T, &FlatEntry, bool) -> Box<dyn Widget> + 'static, ) -> Self`

Create a new TreeView backed by a `TreeModel<T>`.

The delegate receives `(&item, &FlatEntry, selected)` and returns a
boxed widget. The `FlatEntry` provides `depth`, `has_children`, and
`is_expanded` for rendering indentation and expand/collapse toggles.

#### `pub fn new_with_context( model: TreeModel<T>, delegate: impl Fn(&T, &FlatEntry, bool, &TreeRowContext<'_, T>) -> Box<dyn Widget> + 'static, ) -> Self`

Like `new`, but the delegate also receives a
`TreeRowContext` from which `.toggle_callback()` can be
pulled in a single line — eliminating the need to manually
clone the slice handle outside the closure.

```rust
# use bastyde_widgets::{TreeView, StandardTreeItem};
# use bastyde_data::TreeModel;
# use bastyde_i18n::lit;
# struct Item { title: String }
# let model: TreeModel<Item> = TreeModel::new();
let _w = TreeView::new_with_context(model, |item, entry, selected, ctx| {
    Box::new(
        StandardTreeItem::new(lit!(&item.title))
            .from_entry(entry)
            .selected(selected)
            .on_toggle_rc(ctx.toggle_callback())
    )
});
```

#### `pub fn from_source<S: TreeDataSource<Item = T>>( source: S, delegate: impl Fn(&T, &TreeRow, bool) -> Box<dyn Widget> + 'static, ) -> Self`

Create a TreeView backed by any `TreeDataSource` — an external source of
truth (e.g. an entity store) carrying its own `Key`, so it needs no
`TreeModel` mirror. The delegate receives `(&item, &TreeRow, selected)`;
`TreeRow` exposes `depth` / `has_children` / `is_expanded` and a one-call
chevron `toggle_callback()`. Drop validation + lazy windowing route
through the source's `can_accept` / `accept_drop` / `row_state`.

#### `pub fn from_source_keyed<S: TreeDataSource<Item = T>>( source: S, keyed: KeyedSelectionModel<S::Key>, delegate: impl Fn(&T, &TreeRow, bool) -> Box<dyn Widget> + 'static, ) -> Self where S::Key: ItemKey,`

Like `from_source` but with **keyed** selection: the
`KeyedSelectionModel<S::Key>` tracks selection by source identity, so it
survives expand / collapse / filter / reorder and stays consistent across
two views of the same source. The view stays `TreeView<T>` — the `Key` is
captured here. Pruning consults the source's
`contains_key`, so a
collapsed-but-present node keeps its selection.

#### `pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self`

Set the scroll-chaining behavior at the boundary (default
`OverscrollBehavior::Chain`; `Contain`
disables chaining to an ancestor scrollable).

#### `pub fn item_height(mut self, height: f32) -> Self`

Set the fixed height per row (default 28.0) — the uniform fast
path. Mutually exclusive with `item_height_fn`
and `auto_item_height`; the last mode
setter wins.

#### `pub fn smooth_scrolling(mut self, enabled: bool) -> Self`

Enable or disable animated wheel scrolling (enabled by default).
When disabled, wheel events snap immediately to the new offset.

#### `pub fn smooth_scroll_duration(mut self, duration: Duration) -> Self`

Duration of the smooth scroll animation (default 150 ms).

#### `pub fn scroll_bar_style(mut self, style: ScrollBarMode) -> Self`

How the scroll bar is displayed (default `Permanent`). `Overlay`
and `Thin` float the bar over the content instead of reserving a
layout column for it, mirroring `ScrollArea::scroll_bar_style`.

#### `pub fn item_height_fn(mut self, f: impl Fn(usize) -> f32 + 'static) -> Self`

Per-row heights from a callback over the *flat (visible) index*.
The callback must be pure (same index + same data → same height);
it is re-swept from the first changed flat index on every model
change or expand/collapse. No measurement pass runs.

#### `pub fn auto_item_height(mut self, estimated: f32) -> Self`

Auto-measured row heights: each realized row is measured at the
tree's content width (height-for-width), unrealized rows assume
`estimated`. Scroll anchoring keeps content above the viewport
stationary as estimates are corrected; measured heights above a
toggled row survive expand/collapse (divergence-driven
invalidation).

#### `pub fn row_click_expands(mut self, b: bool) -> Self`

Whether a row-body PointerUp on a branch row auto-toggles its
expansion (default `true`). Set to `false` when the delegate
provides its own chevron tap target (e.g. `StandardTreeItem`)
— without this, the auto-toggle fires in addition to the
chevron's own click and they cancel out, leaving the row
expanded only on body clicks.

#### `pub fn selection(mut self, sel: SelectionModel) -> Self`

Set the index-based selection model (visible positions). For
identity-based selection that survives expand / collapse / filter and
node moves, use `keyed_selection` instead.

#### `pub fn keyed_selection(mut self, keyed: KeyedSelectionModel<NodeId>) -> Self`

Set a keyed selection model (by `NodeId`). Selection is tracked by node
identity, so it survives expand / collapse, filtering, and node moves —
and stays consistent if two views share the model. Pruned of deleted
nodes on each slice change. Mutually exclusive with
`selection` (last one set wins).

#### `pub fn reorderable(mut self, enabled: bool) -> Self`

Enable intra-widget drag reordering.

When enabled, tree rows can be dragged to reparent or reorder them.
Before/Into/After is chosen by where in the row the pointer drops; the
move is cycle-guarded — a drop onto the node itself or into its own
subtree is refused and shows no insertion line. Keyboard equivalent:
Alt+ArrowUp/Down.

#### `pub fn on_activate(mut self, f: impl Fn(usize) + 'static) -> Self`

Set the row-**activation** handler — invoked with the flat row index on a
primary click on the row body, or **Enter** on the focused row.
Activation is distinct from *selection*: arrow-key navigation and
**Space** move / toggle the selection but do **not** activate, so a view
can open/commit a row on a deliberate click/Enter without firing on
every navigation step.

#### `pub fn activate_on(mut self, mode: crate::data_views::ActivateOn) -> Self`

Choose single- vs double-click activation (default
`ActivateOn::DoubleClick` — the cross-platform
convention; pass `SingleClick` for the
KDE/web/Scrivener feel). Enter activates in either mode.

#### `pub fn type_ahead_label(mut self, label: impl Fn(&T) -> String + 'static) -> Self`

Enable **type-ahead** ("type to jump"): typing a printable character
while the tree has keyboard focus jumps the selection to the next
*visible* row whose label starts with the accumulated search term,
wrapping around (Qt `keyboardSearch` / macOS & Windows type-select).
`label(&item)` yields the searchable text; matching is
ASCII-case-insensitive. A pause longer than the
`type_ahead_timeout` starts a fresh term.

#### `pub fn type_ahead_timeout(mut self, timeout: Duration) -> Self`

Reset window between keystrokes before the type-ahead search term
clears (default 500 ms). A zero duration disables type-ahead.

#### `pub fn expand(&self, node: bastyde_data::NodeId)`

Expand a node programmatically. No-op on the `from_source` path (which
owns its own expand state — use the source's `set_expanded`).

#### `pub fn collapse(&self, node: bastyde_data::NodeId)`

Collapse a node programmatically. No-op on the `from_source` path.

#### `pub fn toggle(&self, node: bastyde_data::NodeId)`

Toggle a node's expand/collapse state. No-op on the `from_source` path.

#### `pub fn expand_all(&self)`

Expand all nodes. No-op on the `from_source` path.

#### `pub fn collapse_all(&self)`

Collapse all nodes. No-op on the `from_source` path.

#### `pub fn tree_slice(&self) -> Option<&TreeSlice<T>>`

Access the internal `TreeSlice` (for persistence of expand state).
`None` on the `from_source` path, which has no
`TreeSlice` (the external source owns expand state).
