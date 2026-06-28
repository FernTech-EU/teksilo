<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Fade

`Fade` — a wrapper widget that animates its child between hidden
(opacity 0) and visible (opacity 1) when an external
`Signal<bool>` toggles.

Drives an `opacity: Signal<f32>` ∈ [0, 1] and applies it to its
own subtree via [`BuildContext::set_opacity`]. The framework's
render walker emits `SetOpacity(value)` before this widget's
paint and `RestoreOpacity` afterwards, so the multiplier composes
correctly with ancestor opacity scopes via the canvas's stacked
opacity model.

```ignore
let visible = ctx.signal(false);
ctx.add(Fade::new(visible.clone()).child(tooltip_content));
// ...elsewhere:
visible.set(true);  // fades in over `motion.duration_fast`
```

## Layout semantics

`Fade` does not change layout. The wrapped child reports its full
natural size at all opacity values, so reserving space for a
to-be-faded-in widget works the same whether the widget is fully
visible or fully hidden.

For overlays where the dismiss should be *deferred* until the
fade-out completes (tooltip / popover / snackbar / dialog),
prefer `OverlayRequest::with_fade`
instead — that path coordinates the dismiss with the tween so the
overlay survives until the opacity reaches zero.

## Reduced motion

Honours `prefers-reduced-motion`: under reduced motion the
opacity snaps to its end value instead of tweening.

## Builder methods at a glance

`child`, `child_id`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/animations/fade/index.html)

## `pub struct Fade`

Wraps a child and animates the entire subtree's opacity between
0 and 1, driven by an external `Signal<bool>`.

```rust
pub struct Fade { /* fields */ }
```

### Methods

#### `pub fn new(visible: impl Into<Prop<bool>>) -> Self`

Build a fade wrapper bound to `visible`. Initially hidden iff
`visible.get()` is `false` at the first `build()`.

Accepts any `Prop<bool>` source — `Signal<bool>`, `Prop<bool>`,
or a plain `bool` (for static "always visible" / "always
hidden" cases without a tween).

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Inline child widget (deferred insertion).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Pre-registered child by `WidgetId`.
