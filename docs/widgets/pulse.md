<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Pulse

`Pulse` — a wrapper widget that pulses its child's opacity between
a `min` and `max` value on a fixed period, sine-shaped.

## Public functions

### `Pulse`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`opacity(min: f32, max: f32)`](#pulse-opacity) |
| | **Builder methods** |
| `Self` | [`period(period: Duration)`](#pulse-period) |
| `Self` | [`child(widget: impl teksilo_core::IntoTeksiChild)`](#pulse-child) |
| `Self` | [`child_opt(widget: Option<impl teksilo_core::IntoTeksiChild>)`](#pulse-child_opt) |

## Detailed description

The classic "blinking red light" / recording-indicator / attention
beacon pattern. The wrapped subtree pulses smoothly (sine
interpolation), giving a breathing-light feel rather than a hard
on/off blink.

```ignore
ctx.add(
    Pulse::opacity(0.3, 1.0)
        .period(Duration::from_millis(1200))
        .child(RectWidget::new().background(Color::RED)),
);
```

#### Layout semantics

Layout-transparent — the child reports its full natural size at
all opacity values. Identical layout footprint to `Fade`.

#### Reduced motion

Honours `prefers-reduced-motion`: skips the per-frame driver and
pins opacity at the midpoint `(min + max) / 2`. The subtree stays
visible at a steady, non-distracting brightness so the indicator
still communicates "active" without animating.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/animations/pulse/index.html)

<a id="pulse"></a>

## `pub struct Pulse`

Wraps a child and pulses its opacity smoothly between `min` and
`max` on a fixed period. Useful for recording indicators,
notification beacons, and attention-grabbing status icons.

```rust
pub struct Pulse { /* fields */ }
```

### Methods

<a id="pulse-opacity"></a>

#### `pub fn opacity(min: f32, max: f32) -> Self`

Wrap a subtree in an opacity pulse between `min` and `max`
(both clamped to `0..=1`). Uses a sine wave so the transitions
at both extremes are smooth, not abrupt.

<a id="pulse-period"></a>

#### `pub fn period(mut self, period: Duration) -> Self`

Override the pulse period (full cycle min → max → min).
Default: `MotionTokens::duration_indeterminate_sweep` (~900 ms),
the same continuous-loop budget the indeterminate progress bar
and spinner use — so a re-themed motion stack stays consistent.

<a id="pulse-child"></a>

#### `pub fn child(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

Inline child widget (deferred insertion).

<a id="pulse-child_opt"></a>

#### `pub fn child_opt(self, widget: Option<impl teksilo_core::IntoTeksiChild>) -> Self`

Attach `widget` when it is `Some`, and do nothing when it is `None`.

The conditional-child form. `teksu!`'s `if` without an `else` lowers to
this, and it is what `cond.then(|| w)` is for in a builder chain. `None`
adds no arena node, so nothing is laid out, painted, or published to the
accessibility tree, and a stack applies no spacing around it.
