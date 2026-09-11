<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Spacer

![Spacer preview](img/spacer.png)

Spacer — an invisible, flexible gap that claims all available space on the
container's main axis.

Place a `Spacer` inside an `HStack` or
`VStack` to push adjacent siblings to opposite
edges; flank a child with two spacers to centre it. A spacer carries flex
weight `1.0` and zero wanted size, so it soaks up leftover slack without
imposing a cross-axis floor. `min_length` sets a hard
minimum so the gap never collapses below a fixed amount under tight layout.

```rust
# use teksilo_widgets::primitives::{HStack, Spacer, TextWidget};
# use teksilo_i18n::lit;
// Title hugs the leading edge, badge is pushed to the trailing edge.
let _row = HStack::new()
    .child(TextWidget::new(lit!("Title")))
    .child(Spacer::new())
    .child(TextWidget::new(lit!("NEW")));
```

## Density

The picture above is the widget at `TargetDensity::Compact`, the mouse-and-keyboard ladder. Below is the same subject on the same canvas with only the ladder changed, so what moves is the density and nothing else — where the subject no longer fits, that is what the denser targets cost it at that size. See `docs/density-and-targets.md`.

**Touch**

![Spacer at Touch density](img/spacer-touch.png)

## Builder methods at a glance

`min_length`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/spacer/index.html)

## `pub struct Spacer`

An invisible, flexible gap that claims a container's leftover main-axis space.

```rust
pub struct Spacer { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create a spacer with no minimum length (collapses fully when the
container has no slack to give).

#### `pub fn min_length(mut self, min: f32) -> Self`

Set a hard floor, in logical pixels, on the spacer's main-axis size.

The container still adds its slack share on top; the floor only matters
when the container is too cramped to grant any slack. The cross axis is
unaffected, so a horizontal spacer never inflates its stack's height.
