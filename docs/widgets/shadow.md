<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# AttachedSide

Layered drop-shadow helper for elevated surfaces.

Composes two `Shadow`s underneath a rounded rect:
- `outer` — the wide soft halo (typically `theme.shape.shadow_*`).
- `inner` — the sharp short-blur rim that gives the surface a clearly
  "lifted" edge instead of a vague glow (typically the matching
  `theme.shape.shadow_inner_*`).

The `inner` token's geometry (`offset_y`, `blur`, `color.rgb`) is used
verbatim. Only `color.a` is modulated: the painted alpha is
`density × inner.color.a()`, with `density ∈ [0.0, 1.0]` provided by
the per-component `shadow_density` field. This keeps every visual
knob in the theme while letting individual surfaces dial intensity.

Common density presets:
- `1.0` — tooltips (full inner-rim alpha, punchy "lift").
- `~0.5` — cards, popovers, menus (moderate).
- `0.0` — disable inner rim entirely (single-layer outer only).

## Attached side

Popovers, menus and combo-box dropdowns sit *attached* to the widget
that opened them. On the side that touches the trigger, drawing a
halo would visually cut the surface off from its anchor. Pass an
`AttachedSide` to suppress shadow on that side.

```ignore
// Typical usage inside a custom widget's paint() method:
use bastyde_widgets::shadow::{paint_layered_shadow, DENSITY_SURFACE};
paint_layered_shadow(
    canvas, bounds, radius,
    &ctx.theme.shape.shadow_sm,
    &ctx.theme.shape.shadow_inner_sm,
    DENSITY_SURFACE,
    None,
);
```

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/shadow/index.html)

## `pub const DENSITY_TOOLTIP`

Inner-rim alpha multiplier for tooltips — full intensity for maximum lift.

```rust
pub const DENSITY_TOOLTIP: f32 = 1.0;
```

## `pub const DENSITY_SURFACE`

Inner-rim alpha multiplier for cards, popovers, and menus — moderate lift.

```rust
pub const DENSITY_SURFACE: f32 = 0.5;
```

## `pub const DENSITY_DIALOG`

Inner-rim alpha multiplier for snackbars and dialogs — subtle lift.

```rust
pub const DENSITY_DIALOG: f32 = 0.3;
```

## `pub enum AttachedSide`

Which geometric edge of the surface is attached to its trigger and
should have shadow drawing suppressed on that side. Geometric (Top
/ Bottom / Left / Right), not RTL-aware — callers working in
Leading/Trailing terms must resolve to a geometric side using the
active layout direction before calling.

```rust
pub enum AttachedSide { /* variants */ }
```

### Variants

- **`Top`** — Suppress the shadow halo on the top edge (e.g. a dropdown opening downward).
- **`Bottom`** — Suppress the shadow halo on the bottom edge (e.g. a popover opening upward).
- **`Left`** — Suppress the shadow halo on the left edge.
- **`Right`** — Suppress the shadow halo on the right edge.

## `pub fn paint_layered_shadow(...)`

Paint a two-layer drop shadow behind a rounded rect.

The `outer` shadow is drawn unchanged. If `density × inner.color.a()`
is above the sub-perceptual threshold (1/255), the `inner` shadow is
drawn on top with its alpha scaled by `density`. This gives a "lift"
look — a wide soft halo with a sharp close rim.

When `attached` is `Some(side)`, both shadow draws are clipped so
the penumbra on that side is hidden — matching the visual where
the surface is attached to its anchor (popover under its trigger,
dropdown under its combo box, etc.).

If both layers would be sub-perceptual (e.g. theme has zero alphas
or `density` of 0), this function returns without emitting any draw
commands.

```ignore
// In a widget's paint() method:
use bastyde_widgets::shadow::{paint_layered_shadow, AttachedSide, DENSITY_SURFACE};
paint_layered_shadow(
    canvas, bounds, radius,
    &ctx.theme.shape.shadow_sm, &ctx.theme.shape.shadow_inner_sm,
    DENSITY_SURFACE, None,
);
```

```rust
pub fn paint_layered_shadow(
    canvas: &mut Canvas,
    bounds: Rect,
    radius: CornerRadius,
    outer: &Shadow,
    inner: &Shadow,
    density: f32,
    attached: Option<AttachedSide>,
);
```
