<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Repeater

Repeater — non-virtualized dynamic widget list driven by a `ListModel<T>`.

`Repeater` creates one child widget per item in a `ListModel<T>`
using a caller-supplied factory closure. When the model changes (push, remove,
replace-all), the entire child subtree is rebuilt from scratch. Use this for
small collections (tag pills, chapter entries, toolbar button sets) where the
rebuild cost is negligible — typically fewer than 100 items. For large or
potentially unbounded collections, prefer `ListView` which
virtualizes the item pool and only builds visible rows.

Children are arranged in a vertical `VStack`; override the gap with
[`Repeater::spacing`]. Accessibility: the `Repeater` node is hidden from
AT by default so children surface directly into the parent's AT subtree.
Supply [`Repeater::a11y_role`] + [`Repeater::a11y_label`] when the children
genuinely form a named list, menu, or toolbar.

```rust
# use bastyde_widgets::Repeater;
# use bastyde_widgets::primitives::TextWidget;
# use bastyde_data::ListModel;
# use bastyde_i18n::lit;
let model: ListModel<u32> = ListModel::from_vec(vec![1, 2, 3]);
let _w = Repeater::new(model, |_i, _item| {
    Box::new(TextWidget::new(lit!("item")))
})
.spacing(4.0);
```

## Builder methods at a glance

`spacing`, `a11y_role`, `a11y_label`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/repeater/index.html)

## `pub struct Repeater`

A non-virtualized dynamic collection that creates one child widget per item in a `ListModel<T>`.

See the `module-level docs` for usage guidance and an example.

```rust
pub struct Repeater<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new( model: ListModel<T>, factory: impl Fn(usize, &T) -> Box<dyn Widget> + 'static, ) -> Self`

Create a new Repeater backed by a `ListModel<T>`.

The `factory` closure receives `(index, &item)` and returns a boxed widget
for that item.

#### `pub fn spacing(mut self, spacing: f32) -> Self`

Set the spacing between items (default 0.0).

#### `pub fn a11y_role(mut self, role: bastyde_core::accesskit::Role) -> Self`

Expose the Repeater to assistive tech with a specific role
(e.g. `Role::List`, `Role::Menu`, `Role::Toolbar`). Without
this, the Repeater is hidden from AT and its children connect
directly to the parent.

#### `pub fn a11y_label(mut self, label: impl Into<LocalizedString>) -> Self`

Set an accessible name for the Repeater. Only takes effect
alongside `a11y_role`; a hidden Repeater
has no node to attach the name to.
