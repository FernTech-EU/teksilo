<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Pulse

`Pulse` — a wrapper widget that pulses its child's opacity between
a `min` and `max` value on a fixed period, sine-shaped.

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

## Layout semantics

Layout-transparent — the child reports its full natural size at
all opacity values. Identical layout footprint to `Fade`.

## Reduced motion

Honours `prefers-reduced-motion`: skips the per-frame driver and
pins opacity at the midpoint `(min + max) / 2`. The subtree stays
visible at a steady, non-distracting brightness so the indicator
still communicates "active" without animating.

## Builder methods at a glance

`opacity`, `period`, `child`, `child_id`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/animations/pulse/index.html)

## `pub struct Pulse`

Wraps a child and pulses its opacity smoothly between `min` and
`max` on a fixed period. Useful for recording indicators,
notification beacons, and attention-grabbing status icons.

```rust
pub struct Pulse { /* fields */ }
```

### Methods

#### `pub fn opacity(min: f32, max: f32) -> Self`

Wrap a subtree in an opacity pulse between `min` and `max`
(both clamped to `0..=1`). Uses a sine wave so the transitions
at both extremes are smooth, not abrupt.

#### `pub fn period(mut self, period: Duration) -> Self`

Override the pulse period (full cycle min → max → min).
Default: `MotionTokens::duration_indeterminate_sweep` (~900 ms),
the same continuous-loop budget the indeterminate progress bar
and spinner use — so a re-themed motion stack stays consistent.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Inline child widget (deferred insertion).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Pre-registered child by `WidgetId`.
