<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Center

![Center preview](img/center.png)

Center — a single-child wrapper that centers its child within the available
space.

On a **bounded axis** (the parent proposes an exact size), `Center` fills
that dimension and places the child in the middle. On an **unbounded axis**
(the parent leaves it open, as a stack does on its main axis), `Center`
shrink-wraps to the child's natural size rather than collapsing to zero —
this prevents the child from overflowing a prior sibling. `Center` always
reports `flex = 0`, so it never claims slack from a stack's distribution
pass; to center content *within leftover space*, wrap it in an `Expand`:
`Expand::horizontal().child(Center::new().child(w))`.

The child is measured **under the constraint `Center` received** (a
loose-but-bounded proposal, like Flutter's `Center`): rigid children keep
their natural size and are centered, while adaptive children respond to
the bound — an ellipsis `TextWidget` truncates at the slot width instead
of overflowing symmetrically, and wrapping text reports its real wrapped
height.

## When to use

- Center a small widget inside a bounded slot (e.g., an icon in a fixed
  square cell).
- Shrink-wrap and center an element inside a layout that provides an exact
  proposal in both axes.

For claiming *all* remaining stack space and then centering within it, use
`Expand` wrapping `Center` instead.

```rust
# use teksilo_widgets::primitives::{Center, RectWidget};
// Center a rect in the full slot provided by its parent
let _centered = Center::new().child(RectWidget::new());
```

## Builder methods at a glance

`child_id`, `child`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/center/index.html)

## `pub struct Center`

Centers a single child within the space this widget is **given**.

Sizing follows the incoming constraint, per axis: `Center` **fills a
bounded axis** (the tree root, or inside an `Expand` / wrapper that
proposes exact bounds) and **shrink-wraps to the child on an unbounded
axis**. So a bare `Center` does *not* claim slack inside an `HStack` /
`VStack` — those leave their main axis open, and `Center` sizes to its
child there (like Flutter's `Center` / `Align`, or Compose's `Box`),
rather than collapsing to zero and letting the child overflow.

Centering and *expanding* are separate concerns: `Center` reports
`flex = 0` and is a pure alignment wrapper, never a space-claiming one. To
center a child *within the leftover space* of a stack, give it flex with
`Expand` — `Expand::horizontal { Center { child } }` (the analogue of
Flutter's `Expanded(child: Center(...))`).

```rust
pub struct Center { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create a new `Center` with no child attached.

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Set child by pre-registered ID.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Set an inline child widget (deferred insertion).
