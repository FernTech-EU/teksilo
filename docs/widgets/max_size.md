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
# use teksilo_widgets::primitives::{MaxSize, TextWidget};
# use teksilo_i18n::lit;
// Cap a text widget to 240 logical pixels wide.
let _w = MaxSize::width(240.0)
    .child(TextWidget::new(lit!("This text will not exceed 240 dp.")));
```

## Builder methods at a glance

`width`, `height`, `max_width`, `max_height`, `child_id`, `child`, `child_opt`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/max_size/index.html)

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

#### `pub fn max_width(mut self, state: impl Into<Prop<f32>>) -> Self`

Bind max width to a reactive state.

#### `pub fn max_height(mut self, state: impl Into<Prop<f32>>) -> Self`

Bind max height to a reactive state.

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Set child by pre-registered ID.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Set an inline child widget (deferred insertion).

#### `pub fn child_opt(self, widget: Option<impl Widget + 'static>) -> Self`

Attach `widget` when it is `Some`, and do nothing when it is `None`.

The conditional-child form. `teksu!`'s `if` without an `else` lowers to
this, and it is what `cond.then(|| w)` is for in a builder chain. `None`
adds no arena node, so nothing is laid out, painted, or published to the
accessibility tree, and a stack applies no spacing around it.
