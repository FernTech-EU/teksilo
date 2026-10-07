<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Shake

`Shake` — wraps a child and plays a damped horizontal oscillation
whenever an external trigger `Signal<u32>` is bumped. The classic
invalid-input feedback: wrong password, failed form validation,
"no more results" wall.

## Public functions

### `Shake`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(trigger: Signal<u32>)`](#shake-new) |
| | **Builder methods** |
| `Self` | [`amplitude(px: f32)`](#shake-amplitude) |
| `Self` | [`duration(duration: Duration)`](#shake-duration) |
| `Self` | [`cycles(cycles: f32)`](#shake-cycles) |
| `Self` | [`child(widget: impl teksilo_core::IntoTeksiChild)`](#shake-child) |
| `Self` | [`child_opt(widget: Option<impl teksilo_core::IntoTeksiChild>)`](#shake-child_opt) |

## Detailed description

```ignore
let shake_trigger = ctx.signal(0_u32);
ctx.add(
    Shake::new(shake_trigger.clone())
        .child(text_input_field),
);
// ...elsewhere, on validation failure:
shake_trigger.set(shake_trigger.get() + 1);
```

#### Layout semantics

Layout-stable: the wrapper reports the child's full natural size
and clips the oscillating-out-of-bounds excursions on each side.
Siblings don't reflow. The shake is a pure visual offset.

#### Reduced motion

Honours `prefers-reduced-motion`: the trigger no-ops. The widget
is still focusable / interactive — the visual feedback just
doesn't play. Pair with another a11y-friendly cue (red border,
error text) when error state must be communicated.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/animations/shake/index.html)

<a id="shake"></a>

## `pub struct Shake`

Wraps a child and plays a damped horizontal-oscillation shake
each time the trigger signal value changes.

```rust
pub struct Shake { /* fields */ }
```

### Methods

<a id="shake-new"></a>

#### `pub fn new(trigger: Signal<u32>) -> Self`

Build a shake wrapper. Bumping `trigger` (any new value) plays
one shake cycle.

<a id="shake-amplitude"></a>

#### `pub fn amplitude(mut self, px: f32) -> Self`

Peak horizontal offset in logical pixels. Default 8 px.

<a id="shake-duration"></a>

#### `pub fn duration(mut self, duration: Duration) -> Self`

Override the total shake duration. Default:
`MotionTokens::duration_slow` (~300 ms) — the same one-shot
"this should feel deliberate" budget dialogs use.

<a id="shake-cycles"></a>

#### `pub fn cycles(mut self, cycles: f32) -> Self`

Number of full back-and-forth oscillations within `duration`.
Default 4 cycles. Higher = jitterier; lower = wobblier.

<a id="shake-child"></a>

#### `pub fn child(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

Inline child widget (deferred insertion).

<a id="shake-child_opt"></a>

#### `pub fn child_opt(self, widget: Option<impl teksilo_core::IntoTeksiChild>) -> Self`

Attach `widget` when it is `Some`, and do nothing when it is `None`.

The conditional-child form. `teksu!`'s `if` without an `else` lowers to
this, and it is what `cond.then(|| w)` is for in a builder chain. `None`
adds no arena node, so nothing is laid out, painted, or published to the
accessibility tree, and a stack applies no spacing around it.
