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
# use teksilo_widgets::TreeView;
# use teksilo_widgets::primitives::{HStack, Padding, TextWidget};
# use teksilo_data::TreeModel;
# use teksilo_i18n::lit;
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

`toggle_callback`, `slice_handle`, `node_id`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/tree_view/index.html)

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

#### `pub fn toggle_callback(&self) -> std::rc::Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>`

Toggle callback for this row's chevron. Wires in one line:
`.on_toggle_rc(ctx.toggle_callback())`.

#### `pub fn slice_handle(&self) -> TreeSliceHandle<T>`

Cloned handle to the slice — call `.toggle_expand(node)`,
`.expand(node)`, `.collapse(node)` directly.

#### `pub fn node_id(&self) -> teksilo_data::NodeId`

The `NodeId` of this row in the backing `TreeModel`.

## `pub struct TreeView`

```rust
pub struct TreeView<T: 'static> { /* fields */ }
```
