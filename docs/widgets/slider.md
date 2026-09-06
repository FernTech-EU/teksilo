<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Slider

![Slider preview](img/slider.png)

Slider — a draggable value selector bound to a `Signal<f32>`.

The widget owns all input handling: pointer drag (click-to-jump and
thumb-drag), the keyboard, and the `Increment` / `Decrement` /
`SetValue` accessibility actions. All visual chrome is delegated to a
`SliderStyle` implementation; the
IntUI default ships out of the box and is also the theme-wide slot
override target (`theme.style_slots.slider`).

## Keyboard

- `ArrowRight` / `ArrowUp` and `ArrowLeft` / `ArrowDown` — one
  `step`, defaulting to 1 % of the range.
- `PageUp` / `PageDown` — one `page_step`,
  defaulting to ten times the step and so to 10 % of the range. That
  is `QAbstractSlider::pageStep`, `GtkScale`'s page increment, and
  what `<input type=range>` gives in both WebKit and Blink.
- `Home` / `End` — the minimum and the maximum.
- A chord holding `Ctrl`, `Alt` or `Super` is not the slider's and
  falls through to the application; `Shift` does not change the step.

The chord table is shared with every other bounded-scalar control; see
`docs/range-keyboard.md`.

## Accessibility

Exposes `Role::Slider` with numeric value, min, max, step, the page
distance as `numeric_value_jump`, and orientation. Screen readers
announce the current value on every change. `SetValue` accepts a
number or a numeric string and snaps it to `step`, so an assistive
technology's write lands on the same grid a drag does — AT-SPI's
`Value.SetCurrentValue` is how Orca sets a slider, and macOS gates
`setAccessibilityValue:` settability on the action being advertised.
The focus ring follows the `:focus-visible` heuristic — visible after
keyboard interaction, invisible after a pointer tap.

```rust
# use teksilo_core::signal::Signal;
# use teksilo_widgets::Slider;
let volume = Signal::new(0.5_f32);
let _w = Slider::new(volume, 0.0, 1.0).step(0.05);
```

## Builder methods at a glance

`step`, `page_step`, `orientation`, `enabled`, `variant`, `tick_count`, `style`, `label`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/slider/index.html)

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

#### `pub fn page_step(mut self, page_step: f32) -> Self`

Set the step size for `PageUp` / `PageDown`. When unset, ten times the
effective `step` — so with the default step of 1 % of the
range a page is 10 % of it, which is what `QAbstractSlider::pageStep`,
`GtkScale`'s page increment and `<input type=range>` in both WebKit and
Blink all give, and the same `10 x` rule
`SpinBox::page_step` uses.

#### `pub fn orientation(mut self, orientation: Orientation) -> Self`

Set the slider orientation (`Horizontal` by default). Vertical
sliders map Up/Down arrow keys to increase/decrease.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Set the enabled state, statically or reactively. Forwarded to
the arena at build time via
`ctx.enabled_when(slider_id, self.enabled.clone())`.

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

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain single-line tooltip shown after a hover delay.
Mutually exclusive with `rich_tooltip`,
`rich_tooltip_content`, and
`composite_tooltip` — the last setter
wins and clears the others.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip driven by a registry key. The registry
entry supplies title, body markup, optional shortcut chip and
cascade links. Mutually exclusive with the other tooltip setters.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip from an inline `TooltipContent`
value, bypassing the registry lookup. Mutually exclusive with the
other tooltip setters.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip whose body is an arbitrary widget tree.
Uses the heavier `tooltip_delay_heavy` delay. Mutually exclusive
with the other tooltip setters.
