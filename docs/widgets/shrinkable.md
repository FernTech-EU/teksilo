<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Shrinkable

Shrinkable — a layout modifier that allows its child to compress under an over-constraint.

By default every widget is rigid: when a stack runs out of main-axis room, rigid
children keep their wanted size and overflow the bounds. `Shrinkable` opts a child
into the over-constraint distribution: the stack divides any deficit across all
shrinkable children proportional to their `shrink` weight,
never below the `min_width` / `min_height`
floor set here.

`Shrinkable` is the shrink counterpart to
`Expand`: while `Expand` claims leftover slack
(grow), `Shrinkable` absorbs excess pressure (shrink). The two are independent
— a child can both grow on surplus and shrink on deficit by wrapping with
`Shrinkable` and setting a non-zero `flex` on the inner widget.

## When to use

- A long text label that should ellipsize before a rigid icon/badge loses space.
- A thumbnail image column that may compress while a fixed sidebar stays at full width.
- "Compress A before B": give A `Shrinkable`, leave B rigid (`shrink = 0`).

```rust
# use bastyde_widgets::primitives::{HStack, Shrinkable, TextWidget};
# use bastyde_i18n::lit;
// The label shrinks as far as 48 dp; the button stays rigid.
let _row = HStack::new()
    .child(Shrinkable::new().min_width(48.0)
        .child(TextWidget::new(lit!("A long label that may compress")).single_line()))
    .child(TextWidget::new(lit!("Rigid")));
```

## Builder methods at a glance

`shrink`, `min_width`, `min_height`, `child`, `child_id`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/primitives/shrinkable/index.html)

## `pub struct Shrinkable`

Layout modifier that lets its child be **compressed** when a stack is
over-constrained — the shrink counterpart to `Expand`.

By default widgets do not shrink: when an `HStack`/`VStack` runs out of
room, rigid children keep their wanted size and overflow. Wrap a child in
`Shrinkable` to opt it into compression: the parent distributes any deficit
across shrinkable children proportional to their shrink weight, never below
the floor set here.

```rust
# use bastyde_widgets::primitives::{HStack, Shrinkable, TextWidget, IconWidget};
# use bastyde_i18n::lit;
# let long_label = TextWidget::new(lit!("A very long label that may need to shrink"));
# let icon = IconWidget::chevron_right(16.0);
// The label gives up space before the (rigid) icon when the row is narrow:
let _w = HStack::new()
    .child(Shrinkable::new().min_width(40.0).child(long_label))
    .child(icon); // rigid — never shrinks
```

`Shrinkable` preserves its child's grow weight (`flex`) and cross size, so a
child can both grow on surplus and shrink on a deficit. It forwards the
parent's proposal to the child unchanged; when the stack compresses it, the
child is re-laid-out at the smaller size (so e.g. a wrapped-text child
re-wraps and reports its taller height via the height-for-width pass).

**Floor caveat.** The default floor is `0` on both axes, which lets the
child shrink to nothing. Set `min_width` /
`min_height` to a sensible minimum — the caller owns
this choice (unlike the stock height-stable widgets, which report their own
natural floor).

```rust
pub struct Shrinkable { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

A shrinkable wrapper with shrink weight `1.0` and a zero floor.

#### `pub fn shrink(mut self, weight: f32) -> Self`

Set the shrink weight (relative share of an over-constraint deficit this
child absorbs). Clamped to `>= 0`; `0` makes the child rigid again.

#### `pub fn min_width(mut self, min: f32) -> Self`

Set the minimum width the child may be compressed to.

#### `pub fn min_height(mut self, min: f32) -> Self`

Set the minimum height the child may be compressed to.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Wrap an inline child widget (deferred insertion).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Wrap a pre-registered child by id.
