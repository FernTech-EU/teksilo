<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TouchTarget

`TouchTarget` — the last resort of the three hit-targeting mechanisms:
the one that actually moves things.

## Builder methods at a glance

`size`, `reserve_space`, `child`, `child_opt`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/touch_target/index.html)

## `pub struct TouchTarget`

Give an undersized control a conforming touch target, growing the layout
around it when nothing cheaper will do — and **only at
`TargetDensity::Touch`**.

# When you need this, and when you do not

Teksilo has three ways to make a target reachable, and they are ordered by
how much they disturb:

1. **`Widget::hit_outset`** — a thin grip claims the space around it.
   Hit-only; nothing moves. This is what a splitter gutter or a column
   resize strip uses.
2. **The miss-only slop pass** — an isolated small control catches a near
   miss. Hit-only; nothing moves. This is what a radio dot or a chart mark
   uses.
3. **`TouchTarget`** — this. The control genuinely needs *room*, because the
   thing beside it is also a target and there is no space to borrow. It
   changes layout, so siblings reflow.

Reach for (3) only when (1) and (2) cannot work: when a control sits in a
tight row of other controls, so widening its hit area would take presses
from its neighbours rather than from empty space. Everything else the
density sweep already handles by projecting the recipes.

# Why Touch only

At `Compact` and `Comfortable` this wrapper is the **identity**: it reports
its child's own response, unchanged, and adds no hit outset. Compact is the
density every existing layout was designed at and every layout golden was
recorded at, and `Comfortable` is served by the recipes' own density
projection, which raises a control's *own* dimensions rather than padding
around it. `Touch` is the ladder where a 24 dp control still falls 20 dp
short of the target and no recipe can close the gap from inside.

```ignore
// A 16 dp close affordance in a dense tab strip: at Touch it is given a
// 44 dp slot and centred in it; at Compact nothing changes at all.
TouchTarget::new().child(close_button)
```

# `reserve_space`

* `reserve_space(true)` (**the default**) — the slot reports at least
  `size` on both axes and centres the child in it. Siblings reflow.
* `reserve_space(false)` — the slot reports the child's own size and
  declares a `Widget::hit_outset` that brings the *hit* area up to `size`
  instead. Nothing moves; the target overlaps whatever is beside it. Use it
  when the row has slack in one direction but you cannot spend it.

```rust
pub struct TouchTarget { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

A new wrapper at the density's own `target_size`. Attach content with
`child` or `child`.

#### `pub fn size(mut self, dp: f32) -> Self`

Override the target size, in dp. Defaults to the density's
`InputTokens::target_size` (44 dp at Touch).

#### `pub fn reserve_space(mut self, reserve: bool) -> Self`

Whether the slot takes the room it needs (`true`, the default) or widens
only the hit area (`false`). See the type docs.

#### `pub fn child(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

Wrap an inline widget.

#### `pub fn child_opt(self, widget: Option<impl teksilo_core::IntoTeksiChild>) -> Self`

Attach `widget` when it is `Some`, and do nothing when it is `None`.

The conditional-child form. `teksu!`'s `if` without an `else` lowers to
this, and it is what `cond.then(|| w)` is for in a builder chain. `None`
adds no arena node, so nothing is laid out, painted, or published to the
accessibility tree, and a stack applies no spacing around it.
