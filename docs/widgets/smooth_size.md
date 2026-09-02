<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SmoothSize

`SmoothSize` — auto-sizes the slot to fit the child's intrinsic
size, but tweens the change instead of jumping. The "empty panel
that suddenly must grow gracefully to accept new content" pattern.

```ignore
ctx.add(
    SmoothSize::new()
        .axes(SmoothSizeAxes::Both)
        .child(Panel::new().child(content_signal)),
);
```

For *explicit* size animation (target is a numeric signal you
already drive, e.g. a sidebar width), use the existing
`FixedSize::new().width(animated_signal)` + `Signal::animate_to`
pattern instead — that path doesn't need to measure the child every
frame.

## Layout semantics

- The wrapper measures the child's natural size at the proposal
  each layout pass.
- When the natural size differs from the current animation target
  (above 0.5 px), kicks off a new tween.
- `size_that_fits` returns the *current animated value* — what the
  wrapper actually occupies right now, not the target.
- The child is always laid out at its full natural size and clipped
  to the wrapper's smaller animated bounds. Same trick as
  `Collapse` — the child's own internal layout
  doesn't reflow each frame, only the clip rect changes.

## Reduced motion

Honours `prefers-reduced-motion`: snaps to the natural size each
layout pass instead of tweening.

## Builder methods at a glance

`axes`, `duration`, `easing`, `child`, `child_id`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/animations/smooth_size/index.html)

## `pub enum SmoothSizeAxes`

Which axes participate in the size tween. Use `Width` or `Height`
to leave the other axis tracking the child's natural size
instantly.

```rust
pub enum SmoothSizeAxes { /* variants */ }
```

### Variants

- **`Width`** — Animate width changes only; height snaps to natural immediately.
- **`Height`** — Animate height changes only; width snaps to natural immediately.
- **`Both`** — Animate both width and height changes. Default.

## `pub struct SmoothSize`

Wraps a child widget and animates the wrapper's reported size toward
the child's current natural size whenever that size changes.

```rust
pub struct SmoothSize { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

New wrapper. Both axes animate by default.

#### `pub fn axes(mut self, axes: SmoothSizeAxes) -> Self`

Restrict the tween to one axis (the other tracks the child's
natural size instantly).

#### `pub fn duration(mut self, duration: Duration) -> Self`

Override the tween duration. Default: `MotionTokens::duration_normal`.

#### `pub fn easing(mut self, easing: Easing) -> Self`

Override the easing curve. Default: `MotionTokens::easing_standard`.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Inline child widget (deferred insertion).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Pre-registered child by `WidgetId`.
