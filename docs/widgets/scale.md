<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Scale

`Scale` — wraps a child and animates a uniform 2D scale on its
entire subtree when an external `Prop<bool>` toggles. Drives a
`progress: Signal<f32>` ∈ [0, 1] (0 = invisible, 1 = at rest) and
applies it as a centered (or origin-pivoted) scale transform via
`BuildContext::set_transform` — the renderer's transform stack
composes it onto the subtree.

```ignore
let visible = ctx.signal(false);
ctx.add(Scale::new(visible.clone()).child(card));
visible.set(true);   // scale-in around the slot center
```

## Two layout modes

- **Visual-only (default)** — `reflow=false`. The slot stays at the
  child's natural size at all scale values; only the *visual content*
  shrinks/grows around the chosen origin. Use for: overlay enter/exit,
  "boop" feedback on a Card, focus emphasis. Pair with `Center`
  origin (the default).
- **Reflow** — `.reflow(true)`. The wrapper's `layout_response`
  returns `child_size * progress`, so siblings reflow as the child
  shrinks to nothing. The visual content scales by the same factor,
  fitting exactly within the shrunken slot. Use for: a Card that
  disappears by shrinking with surrounding cards filling the gap.
  Pair with `TopLeading` origin (so the visual stays anchored at
  the slot's top-left as it shrinks — otherwise the visual drifts
  while the slot shrinks).

## Why this isn't just `Collapse`

`Collapse` animates only one axis (height by default) and "wipes"
content via clipping — text inside stays at full size, only the
visible portion shrinks. `Scale` shrinks uniformly on both axes,
and text/icons visually get smaller. Different visual vocabulary,
different use cases.

## Reduced motion

Honours `prefers-reduced-motion`: snaps progress to its end value
(visible / hidden) instead of tweening.

## Builder methods at a glance

`reflow`, `origin`, `duration`, `easing`, `child`, `child_id`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/animations/scale/index.html)

## `pub enum ScaleOrigin`

Pivot point for the scale matrix, expressed relative to the
wrapper's slot rectangle.

```rust
pub enum ScaleOrigin { /* variants */ }
```

### Variants

- **`Center`** — Scale around the centre of the slot. Default for visual-only mode.
- **`TopLeading`** — Pin the top-leading corner; content grows/shrinks toward the bottom-trailing.
- **`TopTrailing`** — Pin the top-trailing corner; content grows/shrinks toward the bottom-leading.
- **`BottomLeading`** — Pin the bottom-leading corner; content grows/shrinks toward the top-trailing.
- **`BottomTrailing`** — Pin the bottom-trailing corner; content grows/shrinks toward the top-leading.

## `pub struct Scale`

Wraps a child widget and animates a uniform 2D visual scale on its
subtree when an external `Prop<bool>` toggles between visible and hidden.

```rust
pub struct Scale { /* fields */ }
```

### Methods

#### `pub fn new(visible: impl Into<Prop<bool>>) -> Self`

Create a scale wrapper bound to `visible`; accepts a static `bool`
or a reactive `Signal<bool>`. Defaults: visual-only (no layout
reflow), `Center` origin, `MotionTokens::duration_normal` +
`easing_standard`.

#### `pub fn reflow(mut self, reflow: bool) -> Self`

When `true`, the wrapper's reported size shrinks with progress
(siblings reflow). Pair with `.origin(ScaleOrigin::TopLeading)`
for the "card removal" pattern. Default: `false` (visual-only).

#### `pub fn origin(mut self, origin: ScaleOrigin) -> Self`

Pivot point for the scale matrix. Default `Center` for visual-
only mode; consider `TopLeading` when `reflow=true`.

#### `pub fn duration(mut self, duration: Duration) -> Self`

Override the tween duration. Default: `MotionTokens::duration_normal`.

#### `pub fn easing(mut self, easing: Easing) -> Self`

Override the easing. Default: `MotionTokens::easing_standard`.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Inline child widget (deferred insertion).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Pre-registered child by `WidgetId`.
