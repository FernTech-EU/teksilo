<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# MinSize

MinSize — a layout modifier that ensures a child reaches a minimum width and/or height.

The child's reported size is clamped upward so it never falls below the
configured minimum on each constrained axis. The minimum is also forwarded
as part of the clamped proposal so that wrap-aware children (e.g. a
multi-line `TextWidget`) measure against the constraint they will actually
be placed into. Axes with no minimum set are passed through unchanged.

`MinSize` propagates the child's `flex` and `shrink` weights so that a
`Spacer` or `Expand` inside `MinSize` still participates in stack
slack-distribution; the child's own compression floor is composed with
the `MinSize` floor.

For the inverse operation (capping a maximum size) see `MaxSize`.

```rust
# use bastyde_widgets::primitives::{MinSize, icon_widget::IconWidget};
// Guarantee a 44×44 dp tap target around a 20 dp icon.
let _tap_target = MinSize::new(44.0, 44.0)
    .child(IconWidget::checkmark(20.0));
```

## Builder methods at a glance

`width`, `height`, `min_width`, `min_height`, `child_id`, `child`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/primitives/min_size/index.html)

## `pub struct MinSize`

Layout modifier that enforces a minimum width and/or height on a single child widget.

Constraints can be static or bound to a reactive `Signal<f32>` for dynamic resizing.

```rust
pub struct MinSize { /* fields */ }
```

### Methods

#### `pub fn new(width: f32, height: f32) -> Self`

Enforce a minimum on both axes: the child's width will be at least `width` and its height at least `height`.

#### `pub fn width(width: f32) -> Self`

Enforce a minimum only on the width axis; the height axis is unconstrained by this modifier.

#### `pub fn height(height: f32) -> Self`

Enforce a minimum only on the height axis; the width axis is unconstrained by this modifier.

#### `pub fn min_width(mut self, state: impl Into<Prop<f32>>) -> Self`

Bind min width to a reactive state.

#### `pub fn min_height(mut self, state: impl Into<Prop<f32>>) -> Self`

Bind min height to a reactive state.

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Set child by pre-registered ID.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Set an inline child widget (deferred insertion).
