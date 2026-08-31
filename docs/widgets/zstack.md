<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ZStack

![ZStack preview](img/zstack.png)

ZStack — a layout container that layers children on top of each other.

The container sizes itself to the maximum width and maximum height across
all children, measured at an unspecified proposal so background rects do not
inflate the size. **Height additionally takes a width-bounded query** when the
parent bound the width, so a wrapping child reports the height it will really
occupy rather than a single line; see `layout_response` for why that query is
width-only. Each child is then offered the full container bounds and
positioned according to the container-level `Alignment` (default: `CENTER`);
individual children can override alignment via `WidgetTree::set_alignment`.

The primary use-cases are layered UIs — a background `RectWidget` beneath
a `TextWidget`, a floating badge over a button icon — and card-like
compositions where a paint layer and a content layer share the same bounds.
Children that expand to fill their proposal (e.g. `RectWidget`) fill the
full ZStack area; children with fixed intrinsic sizes are positioned by
alignment.

Propagates shrink weight and minimum size when any child opts in, so
wrapping a shrinkable single-line label in a `ZStack` stays shrinkable.

```rust
# use teksilo_widgets::primitives::{ZStack, TextWidget};
# use teksilo_widgets::RectWidget;
# use teksilo_i18n::lit;
# use teksilo_tokens::SurfaceRole;
let _card = ZStack::new()
    .child(RectWidget::new().background(SurfaceRole::Raised))
    .child(TextWidget::new(lit!("Hello")));
```

## Builder methods at a glance

`alignment`, `add_child`, `child`, `children`, `child_opt`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/primitives/zstack/index.html)

## `pub struct ZStack`

A layout container that stacks children on top of each other.
Size = max of children sizes. Children are positioned according to
the container's `Alignment` (default: center), with per-child overrides.

```rust
pub struct ZStack { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty `ZStack` with center alignment.

#### `pub fn alignment(mut self, alignment: Alignment) -> Self`

Set the alignment applied to every child that does not have a
per-child override set via `WidgetTree::set_alignment`.

#### `pub fn add_child(mut self, id: WidgetId) -> Self`

Add a pre-registered child by ID.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Add an inline child widget (deferred insertion).

#### `pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self`

Add multiple inline children from an iterator.

#### `pub fn child_opt(mut self, widget: Option<impl Widget + 'static>) -> Self`

Conditionally add a child. No-op if None.
