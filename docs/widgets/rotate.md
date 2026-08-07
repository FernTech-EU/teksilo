<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Rotate

`Rotate` — wraps a child and applies a 2D rotation to its entire
subtree, driven by an external `Prop<f32>` of radians. Layout-
stable: the wrapper reports the child's natural size at all
angles; only the visual content rotates within the slot.

```ignore
let angle = ctx.animated_signal(0.0);
ctx.add(Rotate::new(angle.clone()).child(chevron));
// Animate to 90° on expand:
angle.animate_to(std::f32::consts::FRAC_PI_2, Duration::from_millis(150), Easing::EaseOut);
```

No internal animation — the caller owns the angle signal and pairs
it with `Signal::animate_to` (or `ctx.animate()`) for animated
rotations. This keeps the widget composable: bind it to interaction
state for hover-on rotation, to an animated signal for spinning
loaders, to a constant for static decorative rotation.

Use cases: animated chevrons (the disclosure-state pattern, today
faked by visibility-toggling two static chevron icons), spinning
loaders not covered by `Spinner`, "shake your
head no" rotation feedback, dial controls.

## Reduced motion

Rotate doesn't introduce motion — it just applies whatever the
caller's angle signal currently holds. Reduced-motion handling
belongs at the *caller's* `animate_to` site (use `to_or_snap` or
gate the animation behind `prefers_reduced_motion`).

## Builder methods at a glance

`origin`, `child`, `child_id`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/animations/rotate/index.html)

## `pub struct Rotate`

Wraps a child widget and rotates its entire subtree by an
externally-driven angle in radians.

```rust
pub struct Rotate { /* fields */ }
```

### Methods

#### `pub fn new(angle: impl Into<Prop<f32>>) -> Self`

Create a rotate wrapper bound to `angle` (radians); accepts a
static `f32` or a reactive `Signal<f32>`. Default pivot: `Center`.

#### `pub fn origin(mut self, origin: ScaleOrigin) -> Self`

Pivot point for the rotation. Default `Center`.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Inline child widget (deferred insertion).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Pre-registered child by `WidgetId`.
