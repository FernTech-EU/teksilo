<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Shake

`Shake` — wraps a child and plays a damped horizontal oscillation
whenever an external trigger `Signal<u32>` is bumped. The classic
invalid-input feedback: wrong password, failed form validation,
"no more results" wall.

```ignore
let shake_trigger = ctx.signal(0_u32);
ctx.add(
    Shake::new(shake_trigger.clone())
        .child(text_input_field),
);
// ...elsewhere, on validation failure:
shake_trigger.set(shake_trigger.get() + 1);
```

## Layout semantics

Layout-stable: the wrapper reports the child's full natural size
and clips the oscillating-out-of-bounds excursions on each side.
Siblings don't reflow. The shake is a pure visual offset.

## Reduced motion

Honours `prefers-reduced-motion`: the trigger no-ops. The widget
is still focusable / interactive — the visual feedback just
doesn't play. Pair with another a11y-friendly cue (red border,
error text) when error state must be communicated.

## Builder methods at a glance

`amplitude`, `duration`, `cycles`, `child`, `child_id`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/animations/shake/index.html)

## `pub struct Shake`

Wraps a child and plays a damped horizontal-oscillation shake
each time the trigger signal value changes.

```rust
pub struct Shake { /* fields */ }
```

### Methods

#### `pub fn new(trigger: Signal<u32>) -> Self`

Build a shake wrapper. Bumping `trigger` (any new value) plays
one shake cycle.

#### `pub fn amplitude(mut self, px: f32) -> Self`

Peak horizontal offset in logical pixels. Default 8 px.

#### `pub fn duration(mut self, duration: Duration) -> Self`

Override the total shake duration. Default:
`MotionTokens::duration_slow` (~300 ms) — the same one-shot
"this should feel deliberate" budget dialogs use.

#### `pub fn cycles(mut self, cycles: f32) -> Self`

Number of full back-and-forth oscillations within `duration`.
Default 4 cycles. Higher = jitterier; lower = wobblier.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Inline child widget (deferred insertion).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Pre-registered child by `WidgetId`.
