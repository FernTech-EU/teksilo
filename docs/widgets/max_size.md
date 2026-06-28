<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# MaxSize

MaxSize — a layout modifier that caps a child to a maximum width and/or height.

The child is proposed the lesser of the parent's proposal and the configured
maximum on each axis; the reported size is then clamped again so a child that
intrinsically overshoots the cap is always contained. Axes with no maximum set
are passed through unchanged.

`MaxSize` clips its child when a maximum is active (`clips_children() == true`)
so content that still overflows after layout does not bleed into adjacent widgets.
Maximum values can be static or bound to a reactive `Signal<f32>`
for animated or data-driven constraints.

For the inverse operation (ensuring a minimum size) see `MinSize`.

```rust
# use bastyde_widgets::primitives::{MaxSize, TextWidget};
# use bastyde_i18n::lit;
// Cap a text widget to 240 logical pixels wide.
let _w = MaxSize::width(240.0)
    .child(TextWidget::new(lit!("This text will not exceed 240 dp.")));
```

## Builder methods at a glance

`width`, `height`, `bind_max_width`, `bind_max_height`, `child_id`, `child`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/primitives/max_size/index.html)

## `pub struct MaxSize`

Layout modifier that enforces a maximum width and/or height on a single child widget.

Constraints can be static or bound to a reactive `Signal<f32>` for dynamic resizing.

```rust
pub struct MaxSize { /* fields */ }
```

### Methods

#### `pub fn new(width: f32, height: f32) -> Self`

Cap both axes: the child's width will not exceed `width` and its height will not exceed `height`.

#### `pub fn width(width: f32) -> Self`

Cap only the width axis; the height axis is unconstrained by this modifier.

#### `pub fn height(height: f32) -> Self`

Cap only the height axis; the width axis is unconstrained by this modifier.

#### `pub fn bind_max_width(mut self, state: impl Into<Prop<f32>>) -> Self`

Bind max width to a reactive state.

#### `pub fn bind_max_height(mut self, state: impl Into<Prop<f32>>) -> Self`

Bind max height to a reactive state.

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Set child by pre-registered ID.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Set an inline child widget (deferred insertion).
