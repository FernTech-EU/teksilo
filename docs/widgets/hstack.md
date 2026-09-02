<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# HStack

![HStack preview](img/hstack.png)

HStack — a horizontal layout container that distributes children left-to-right.

Children are given their intrinsic width and the stack's cross-axis height.
Positive slack (leftover space) is distributed among children that carry a
non-zero `flex` weight (e.g. `Spacer`, `Expand`); negative slack (over-constraint)
is absorbed by children with a non-zero `shrink` weight (e.g. a single-line
`TextWidget`). Vertical alignment defaults to `VAlignment::Center` and can be
overridden per-container or per-child.

For a vertical counterpart see `VStack`.

```rust
# use teksilo_widgets::primitives::{HStack, TextWidget, Spacer};
# use teksilo_i18n::lit;
let _row = HStack::new()
    .spacing(8.0)
    .child(TextWidget::new(lit!("Label")))
    .child(Spacer::new())
    .child(TextWidget::new(lit!("Value")));
```

## Builder methods at a glance

`spacing`, `alignment`, `add_child`, `child`, `children`, `child_opt`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/hstack/index.html)

## `pub struct HStack`

Horizontal layout container that distributes children left-to-right.

Cross-axis (vertical) alignment defaults to `VAlignment::Center` and may be
overridden globally via `alignment` or per-child via
`WidgetTree::set_alignment`.

```rust
pub struct HStack { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty `HStack` with no spacing and `VAlignment::Center`.

#### `pub fn spacing(mut self, spacing: impl Into<Prop<f32>>) -> Self`

Set inter-child spacing. Accepts a static `f32` or a reactive
`Signal<f32>` — use a signal derived from
`ctx.theme_signal()` to track theme-driven spacing changes.

#### `pub fn alignment(mut self, alignment: VAlignment) -> Self`

Set the vertical alignment for children that are shorter than the stack's height.

#### `pub fn add_child(mut self, id: WidgetId) -> Self`

Add a pre-registered child by ID.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Add an inline child widget (deferred insertion).

#### `pub fn children(mut self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self`

Add multiple inline children from an iterator.

#### `pub fn child_opt(mut self, widget: Option<impl Widget + 'static>) -> Self`

Conditionally add a child. No-op if None.
