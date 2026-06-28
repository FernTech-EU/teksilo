<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Unroll

`Unroll` — the horizontal sibling of `Collapse`.

Animates a child's *width* between zero and natural while the child
keeps its full natural layout — the framework's clip pass crops the
overflow, so the visible reveal tracks progress linearly across the
whole duration and the child never reflows mid-animation. This is the
same "lay out full, clip the shrinking axis" trick the docking
`Splitter` uses for its side expand/collapse (`ClipPane`).

Two drivers:

- `Unroll::new(expanded)` — self-animated, like
  `Collapse`. Flips between 0 and natural width over
  `MotionTokens::duration_collapse` whenever `expanded` toggles.
- `Unroll::from_progress(progress)` — driven
  by an external animated `Signal<f32>` ∈ [0, 1]. Use when something
  *else* owns the tween — e.g. an overlay whose deferred dismissal
  rolls the width back into its anchor before going dormant.

The reveal edge is chosen with `reveal_from`:
[`UnrollFrom::Leading`] (default) keeps the leading edge pinned and
grows trailing-ward — the "slide out from a button on the left"
shape; [`UnrollFrom::Trailing`] mirrors it.

Honors `prefers-reduced-motion`: the self-animated driver snaps to
its end value instead of tweening (the external driver's owner is
responsible for its own reduced-motion policy).

```rust
# use bastyde_widgets::animations::{Unroll, UnrollFrom};
# use bastyde_widgets::primitives::TextWidget;
# use bastyde_core::signal::Signal;
# use bastyde_i18n::lit;
let expanded = Signal::new(false);
let _w = Unroll::new(expanded)
    .reveal_from(UnrollFrom::Leading)
    .child(TextWidget::new(lit!("Reveal me")));
```

## Builder methods at a glance

`from_progress`, `child`, `child_id`, `reveal_from`, `progress_signal`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/animations/unroll/index.html)

## `pub enum UnrollFrom`

Which edge stays anchored as the child unrolls.

```rust
pub enum UnrollFrom { /* variants */ }
```

### Variants

- **`Leading`** — Pin the leading edge; reveal trailing-ward (default).
- **`Trailing`** — Pin the trailing edge; reveal leading-ward.

## `pub struct Unroll`

Wraps a child widget and reveals or hides it along the horizontal
axis by animating the wrapper's reported width between zero and the
child's natural width. See the module docs for the two available drivers.

```rust
pub struct Unroll { /* fields */ }
```

### Methods

#### `pub fn new(expanded: Signal<bool>) -> Self`

Self-animated wrapper bound to `expanded`. Initially rolled up
iff `expanded.get()` is `false` at the first `build()`.

#### `pub fn from_progress(progress: Signal<f32>) -> Self`

Externally-driven wrapper. `progress` (an animated 0..1 signal)
is read every layout; the caller owns the tween. Use when an
overlay or other coordinator drives the reveal lifecycle.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Inline child widget (deferred insertion).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Pre-registered child by `WidgetId`.

#### `pub fn reveal_from(mut self, from: UnrollFrom) -> Self`

Set the edge that stays anchored as the child unrolls. Defaults
to [`UnrollFrom::Leading`].

#### `pub fn progress_signal(&self) -> Option<Signal<f32>>`

Return the live progress signal (0.0 = rolled up, 1.0 = fully
unrolled). Returns `None` before the first `build()`. Useful for
tests and external coordinators that need to observe or gate on
the current animated progress.
