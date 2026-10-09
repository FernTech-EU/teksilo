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

## Value readout and reset

`value_tooltip` shows the value as text beside the
thumb — "-3.0 dB" on an equalizer band — while the pointer is over the
slider, while a drag is moving it, and while it has keyboard focus. It is up
at once rather than after a hover delay, follows the thumb wherever the
slider goes, and changes as the value does. It takes no input: a press or a
hover on it belongs to what is under it, and Escape takes it down without
keeping the key from the field the user pressed it for. The same text
becomes the slider's accessible value, beside the number, so a reader that
speaks a value's text says "-3.0 dB" rather than "-3".

`default_value` adds a way back: a double-click on
the slider, or the "Reset to default" accessibility action, puts the value
back to the default, as a user change, so `on_change`
reports it.

```rust
# use teksilo_core::signal::Signal;
# use teksilo_i18n::lit;
# use teksilo_widgets::Slider;
let gain = Signal::new(0.0_f32);
let _band = Slider::new(gain, -12.0, 12.0)
    .step(0.5)
    .label(lit!("Low shelf"))
    .value_tooltip(|db| lit!(format!("{db:+.1} dB")))
    .default_value(0.0);
```

## Touch and pen

A slider is a **continuous manipulator**: the value it produces *is* the
press position, so a finger that lands on it adjusts it — even inside a
scrolling form, and from the first movement rather than after a long-press
timer. Two separate things deliver that. The press *capture* the drag takes
makes the slider the innermost member of the pointer's sequence, which is
what stops an enclosing scroller winning the gesture. `touch_action(NONE)`
is the declaration on top: it forbids every default touch behaviour on the
hit path, which in practice means a **two-contact pinch** started on the
slider never reaches the surface under it. `docs/touch-and-pen.md` §7.3.

`teksilo_core::widget::Widget::target_regions`
reports what the style painted inside the slider's one node: the whole node
as the press surface, and the knob as the grab affordance, sized through the
resolved style's density-aware `thumb_diameter_for`. Nothing else in the
tree can see the knob — it is drawn on the same canvas as the track — so
this is the only way a conformance audit or a coarse-press router learns it
is there.

**Right-to-left.** A horizontal slider's minimum sits at the *leading* edge,
which is the right-hand one in an RTL UI, so both the painted fill and the
position→value map mirror. They were previously mirrored in neither, so an
RTL slider's knob moved away from the finger dragging it.

## Density

The picture above is the widget at `TargetDensity::Compact`, the mouse-and-keyboard ladder. Below is the same subject on the same canvas with only the ladder changed, so what moves is the density and nothing else — where the subject no longer fits, that is what the denser targets cost it at that size. See `docs/density-and-targets.md`.

**Touch**

![Slider at Touch density](img/slider-touch.png)

## Builder methods at a glance

`on_change`, `step`, `page_step`, `orientation`, `enabled`, `variant`, `tick_count`, `style`, `label`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`, `value_tooltip`, `default_value`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/slider/index.html)

## `pub const SLIDER_PART_BODY`

`Widget::target_regions` part id for the whole press surface — the track
plus everything either side of it, which is what a tap or a drag acts on.

```rust
pub const SLIDER_PART_BODY: u16 = 0;
```

## `pub const SLIDER_PART_THUMB`

`Widget::target_regions` part id for the knob.

```rust
pub const SLIDER_PART_THUMB: u16 = 1;
```

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

#### `pub fn on_change( mut self, f: impl Fn(f32, &mut teksilo_core::widget::EventContext) + 'static, ) -> Self`

Run `f` for every value this control produces under the **user's**
hand, with an `EventContext`, so it can do what a bare `Signal` write
cannot (`ctx.send_intent(...)`, opening a window). Fires for a track
click, for each step of a drag, for the arrows, for an assistive
technology's `Increment` / `Decrement` / `SetValue`, and for a reset to
the `default_value`.

**A drag fires this repeatedly** — once per value it actually produces,
not once per pointer sample, since a write that changes nothing reports
nothing. It is still the wrong place for work that should happen once
per interaction: persisting to disk, a network call, an undo entry.
There is no commit-on-release callback yet; observe the signal and do
that work when the value settles.

Does **not** fire for programmatic writes to the bound signal — there is
no event in flight to carry. Observe the signal for that.

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
wins and clears the others. Independent of
`value_tooltip`, which neither replaces nor is
replaced by any of them.

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

#### `pub fn value_tooltip(mut self, format: impl Fn(f32) -> LocalizedString + 'static) -> Self`

Show the value as text by the thumb, formatted by `format`, while the
slider is hovered, dragged, or focused from the keyboard.

Unlike a `tooltip`, the readout is up at once, with no
hover delay; a press does not close it, so it stays up through a drag;
and it moves with the thumb and changes as the value does, without a
rebuild. It sits above a horizontal slider's thumb, or below it near
the top of the window, and beside a vertical slider, on the
inline-start side when there is room, so it covers neither the thumb
nor the track. It is re-placed on every layout pass from where the
slider is then, so it follows a value the application writes, a scroll
that moves the slider and a resize that stretches it.

It takes no input of its own. A press or a hover on it reaches what is
under it, and a press there closes a popover it is outside of. Escape
hides it until the next hover, drag or key, and goes on to whatever
else wanted the key, the focused field included, as a plain tooltip's
Escape does. Focus leaving the slider takes it down only when nothing
else holds it up: the pointer still on the slider keeps it.

The text is also the slider's accessible value, published beside the
number: UI Automation and macOS carry it, so a reader there says
"-3.0 dB" rather than "-3". AT-SPI carries the number alone, so Orca
keeps reading the number.

`format` receives the value the bound signal holds, on every change to
it. It returns a `LocalizedString`, so a `tr!(..)` message
re-resolves on a locale change and `lit!(..)` serves text that is not
translated.

A `tooltip`, rich or composite tooltip set beside it
keeps its hover delay, its place under the slider and the accessible
description it gives, and both show; to name the control in the
readout instead, say so in `format`.

#### `pub fn default_value(mut self, value: f32) -> Self`

Let the user put the value back to `value`: with a double-click on the
slider, or with the "Reset to default" accessibility action, which this
also advertises.

The reset is a user change, written through the same path as a drag or
an arrow key, so `on_change` reports it unless the
value was already there. It is clamped to the range and not snapped to
the `step`: a default between two steps lands where it
was asked to. A double-click's first click jumps the value to where it
landed, as every click on the track does, so `on_change` reports that
value and then the default; further clicks in the same burst leave the
default where it is. A disabled slider ignores the double-click, and
neither offers nor takes the action.

An application's own `.access_custom_action(..)` on the slider is
offered beside the reset, not instead of it.
