<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Slider

Slider — a draggable value selector bound to a `Signal<f32>`.

The widget owns all input handling: pointer drag (click-to-jump and
thumb-drag), keyboard arrows (`ArrowRight`/`ArrowLeft`/`Up`/`Down`,
`Home`, `End`), and `Increment`/`Decrement` accessibility actions.
All visual chrome is delegated to a
`SliderStyle` implementation; the
IntUI default ships out of the box and is also the theme-wide slot
override target (`theme.style_slots.slider`).

## Accessibility

Exposes `Role::Slider` with numeric value, min, max, step, and
orientation. Screen readers announce the current value on every
change. The focus ring follows the `:focus-visible` heuristic —
visible after keyboard interaction, invisible after a pointer tap.

```rust
# use bastyde_core::signal::Signal;
# use bastyde_widgets::Slider;
let volume = Signal::new(0.5_f32);
let _w = Slider::new(volume, 0.0, 1.0).step(0.05);
```

## Builder methods at a glance

`step`, `orientation`, `enabled`, `variant`, `tick_count`, `style`, `label`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/slider/index.html)

## `pub struct Slider`

A draggable value selector bound to a `Signal<f32>` in a continuous
or discrete range. Visual chrome is fully delegated to a
`SliderStyle` implementation.

```rust
pub struct Slider { /* fields */ }
```

### Methods

#### `pub fn new(value: Signal<f32>, min: f32, max: f32) -> Self`

Create a horizontal slider bound to `value` with the given inclusive
range. Use `orientation` to switch to vertical.

#### `pub fn step(mut self, step: f32) -> Self`

Set the discrete step size for keyboard arrows and accessibility
Increment/Decrement actions. When unset, defaults to 1 % of the
range.

#### `pub fn orientation(mut self, orientation: Orientation) -> Self`

Set the slider orientation (`Horizontal` by default). Vertical
sliders map Up/Down arrow keys to increase/decrease.

#### `pub fn enabled(mut self, enabled: bool) -> Self`

Set the initial enabled state. Forwarded to the arena at build
time. Use `ctx.enabled_when(slider_id, signal)` for reactivity.

#### `pub fn variant(mut self, variant: SliderVariant) -> Self`

Pick a Tier-1 design-language variant
(`SliderVariant::Continuous` / `Discrete` / `Range`). The
active `SliderStyle` decides what to do with the hint —
IntUI's default impl paints ticks for `Discrete` and ignores
`Range` (the widget itself doesn't yet wire dual-thumb
behaviour).

#### `pub fn tick_count(mut self, count: u32) -> Self`

Configure the tick count for a `Discrete` slider. The
IntUI default paints `n` evenly spaced tick marks above the
track (or to the leading side for vertical orientation).

#### `pub fn style(mut self, style: impl SliderStyle) -> Self`

Override the active `SliderStyle` for this widget instance
only.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Set an accessible name for the slider, announced by screen readers.
ARIA requires sliders to have a label; when none is set here the
caller is responsible for labelling via a wrapping element.
