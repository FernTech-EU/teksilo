<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Collapse

`Collapse` — a wrapper widget that animates its child between
hidden and natural size when an external `Signal<bool>` toggles.

Drives a `progress: Signal<f32>` ∈ [0, 1] (0 = collapsed,
1 = expanded) and reports its own size as `(natural_w, natural_h *
progress)` while the child lays out at full natural size — the
framework's clip pass crops the overflow. This keeps the animation
visible across the *whole* duration, instead of compressing the
visible portion into the final few milliseconds (which is what
happened when an animated `MaxSize::max_height` slid against a
10000-px sentinel that vastly overshot the child's natural height).

```ignore
let expanded = ctx.signal(false);
ctx.add(Collapse::new(expanded.clone()).child(advanced_settings));
// ...elsewhere:
expanded.set(true);  // animates open over `motion.duration_collapse`
```

Honors `prefers-reduced-motion`: under reduced motion, progress
snaps to its end value instead of tweening.

## Builder methods at a glance

`child`, `child_id`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/animations/collapse/index.html)

## `pub struct Collapse`

Wraps a child and animates it between hidden (progress=0) and
natural size (progress=1), driven by an external `Signal<bool>`.

```rust
pub struct Collapse { /* fields */ }
```

### Methods

#### `pub fn new(expanded: Signal<bool>) -> Self`

Build a collapse wrapper bound to `expanded`. Initially
collapsed iff `expanded.get()` is `false` at the first
`build()`.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Inline child widget (deferred insertion).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Pre-registered child by `WidgetId`.
