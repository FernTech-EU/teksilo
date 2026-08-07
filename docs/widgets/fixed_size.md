<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# FixedSize

FixedSize — a layout modifier that pins a child to its natural size,
optionally overriding one or both dimensions with a reactive value.

Without bindings, `FixedSize` ignores the parent's size proposal and
always reports the child's intrinsic size. This is useful for widgets
that must not be stretched or compressed by their containing stack —
icons, chips, or thumbnails that must stay at their designed size
regardless of the surrounding layout.

With `width` or
`height`, the corresponding dimension is
locked to a reactive `Signal<f32>` value; the signal change triggers a
relayout automatically. Unbound dimensions still fall back to the child's
natural size.

```rust
# use teksilo_widgets::primitives::{FixedSize, RectWidget};
# use teksilo_core::signal::Signal;
let sidebar_width = Signal::new(240.0_f32);
// Pin the sidebar width to a reactive signal
let _sidebar = FixedSize::new()
    .width(sidebar_width)
    .child(RectWidget::new());
```

## Builder methods at a glance

`child_id`, `child`, `width`, `height`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/primitives/fixed_size/index.html)

## `pub struct FixedSize`

Layout modifier that prevents a widget from expanding beyond its natural size,
or constrains it to specific reactive dimensions.

Without bindings, reports the child's natural size (ignoring parent proposal).
With `width`/`height`, constrains to the bound values.

```rust
pub struct FixedSize { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create a `FixedSize` with no child and no dimension bindings; the child's
natural size will be used for both axes.

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Set child by pre-registered ID.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Set an inline child widget (deferred insertion).

#### `pub fn width(mut self, state: impl Into<Prop<f32>>) -> Self`

Bind width to a reactive state. When the state changes, relayout is triggered.

#### `pub fn height(mut self, state: impl Into<Prop<f32>>) -> Self`

Bind height to a reactive state. When the state changes, relayout is triggered.
